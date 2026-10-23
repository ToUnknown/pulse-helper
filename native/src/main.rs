//! Isolated native media process. Private parent/child pipes only, no listener.
//! Protocol v1 is deliberately a single-publisher proof, not a release backend.
#![cfg_attr(target_os = "windows", windows_subsystem = "windows")]
mod helper;
use anyhow::{bail, Context, Result};
use gstreamer as gst;
use gstreamer::prelude::*;
use gstreamer_rtp::{self as rtp, prelude::*};
use gstreamer_sdp as sdp;
use gstreamer_webrtc as rtc;
use serde::Deserialize;
use serde_json::{json, Value};
use std::{
    io::{self, BufRead, Write},
    sync::{Arc, Mutex},
    time::Instant,
};

fn event(value: Value) {
    let mut out = io::stdout().lock();
    let _ = writeln!(out, "{}", value);
    let _ = out.flush();
}

#[derive(Deserialize)]
#[serde(tag = "type", rename_all = "camelCase", deny_unknown_fields)]
enum Command {
    Start {
        monitor: i32,
        #[serde(default)]
        audio: bool,
        #[serde(default)]
        wgc: bool,
    },
    Answer {
        sdp: String,
    },
    Candidate {
        candidate: String,
        #[serde(rename = "sdpMLineIndex")]
        line: u32,
    },
    Stop,
}

struct Sender {
    pipeline: gst::Pipeline,
    transport: gst::Element,
    pending: Vec<(u32, String)>,
    answered: bool,
    _bus: gst::bus::BusWatchGuard,
    timer: Option<gst::glib::SourceId>,
    _timer_resolution: TimerResolution,
    #[cfg(target_os="macos")]
    capture: *mut std::ffi::c_void,
}

// Windows gives timer resolution to the requesting process. Keep this request
// scoped to an active capture, rather than changing the monitor or global settings.
struct TimerResolution;
#[cfg(target_os = "windows")]
#[link(name = "winmm")]
extern "system" {
    fn timeBeginPeriod(period: u32) -> u32;
    fn timeEndPeriod(period: u32) -> u32;
}
impl TimerResolution {
    fn new() -> Self {
        #[cfg(target_os = "windows")]
        unsafe {
            timeBeginPeriod(1);
        }
        Self
    }
}
impl Drop for TimerResolution {
    fn drop(&mut self) {
        #[cfg(target_os = "windows")]
        unsafe {
            timeEndPeriod(1);
        }
    }
}

#[derive(Default)]
struct Counters {
    capture: u64,
    encoded: u64,
    previous_capture: u64,
    previous_encoded: u64,
    last_capture: Option<Instant>,
    gaps_ms: Vec<f64>,
}

impl Sender {
    fn start(monitor: i32, audio: bool, wgc: bool) -> Result<Self> {
        let timer_resolution = TimerResolution::new();

        if !(-1..=15).contains(&monitor) {
            bail!("Invalid display selection");
        }
        #[cfg(target_os="windows")]
        for factory in [
            "d3d11screencapturesrc",
            "d3d11convert",
            "nvd3d11h264enc",
            "webrtcbin",
            "nicesrc",
        ] {
            if gst::ElementFactory::find(factory).is_none() {
                bail!("Required native component unavailable: {factory}");
            }
        }
        // Never fall back silently to a CPU encoder. Keep D3D11 textures resident
        // from DXGI/WGC through GPU resize/color conversion and NVENC.
        #[cfg(target_os="windows")]
        let video = format!(
            concat!(
            "d3d11screencapturesrc name=capture monitor-index={} capture-api={} show-cursor=true ",
            "! video/x-raw(memory:D3D11Memory),framerate=60/1 ",
            "! clocksync ",
            "! queue max-size-buffers=2 max-size-bytes=0 max-size-time=0 leaky=downstream ",
            "! d3d11convert ",
            "! video/x-raw(memory:D3D11Memory),format=NV12,width=1920,height=1080 ",
            "! nvd3d11h264enc name=encoder bitrate=18000 max-bitrate=18000 rc-mode=cbr ",
            "gop-size=60 bframes=0 rc-lookahead=0 zerolatency=true tune=ultra-low-latency ",
            "! video/x-h264,profile=constrained-baseline,stream-format=byte-stream,alignment=au ",
            "! h264parse ! rtph264pay name=videopay pt=96 mtu=1200 config-interval=-1 aggregate-mode=zero-latency ",
            "! application/x-rtp,media=video,encoding-name=H264,payload=96,clock-rate=90000 ",
            "! transport. "
        ),
            monitor,
            if wgc { "wgc" } else { "dxgi" }
        );
        #[cfg(target_os="windows")]
        let sound = if audio {
            format!(concat!(
            "wasapi2src loopback=true loopback-mode=exclude-process-tree loopback-target-pid={} low-latency=true ",
            "! queue max-size-time=100000000 max-size-buffers=0 max-size-bytes=0 leaky=downstream ",
            "! audioconvert ! audioresample ! audio/x-raw,rate=48000,channels=2 ",
            "! opusenc bitrate=128000 frame-size=10 audio-type=restricted-lowdelay ",
            "! rtpopuspay name=audiopay pt=111 mtu=1200 ! application/x-rtp,media=audio,encoding-name=OPUS,payload=111,clock-rate=48000 ",
            "! transport."
        ), std::process::id())
        } else {
            String::new()
        };
        #[cfg(target_os="macos")]
        let video="appsrc name=capture is-live=true format=time max-buffers=3 leaky-type=downstream caps=video/x-raw,format=NV12,width=1920,height=1080,framerate=60/1 ! vtenc_h264_hw name=encoder bitrate=18000 realtime=true allow-frame-reordering=false max-keyframe-interval=60 ! h264parse ! rtph264pay name=videopay pt=96 mtu=1200 config-interval=-1 aggregate-mode=zero-latency ! application/x-rtp,media=video,encoding-name=H264,payload=96,clock-rate=90000 ! transport.";
        #[cfg(target_os="macos")]
        let sound=if audio {"appsrc name=systemaudio is-live=true format=time max-buffers=10 leaky-type=downstream caps=audio/x-raw,format=F32LE,rate=48000,channels=2,layout=interleaved ! audioconvert ! opusenc bitrate=128000 frame-size=10 audio-type=restricted-lowdelay ! rtpopuspay name=audiopay pt=111 mtu=1200 ! application/x-rtp,media=audio,encoding-name=OPUS,payload=111,clock-rate=48000 ! transport."}else{""};
        let pipeline = gst::parse::launch(&format!(
            "webrtcbin name=transport bundle-policy=max-bundle stun-server=stun://stun.cloudflare.com:3478 {video} {sound}"
        ))?.downcast::<gst::Pipeline>().map_err(|_| anyhow::anyhow!("Expected pipeline"))?;
        let transport = pipeline
            .by_name("transport")
            .context("Missing WebRTC transport")?;
        for name in ["videopay", "audiopay"] {
            if let Some(pay) = pipeline.by_name(name) {
                let extension = rtp::RTPHeaderExtension::create_from_uri(
                    "http://www.ietf.org/id/draft-holmer-rmcat-transport-wide-cc-extensions-01",
                )
                .context("Missing transport feedback header extension")?;
                extension.set_id(1);
                pay.emit_by_name::<()>("add-extension", &[&extension]);
            }
        }
        for pad in transport.sink_pads() {
            let transceiver = pad.property::<rtc::WebRTCRTPTransceiver>("transceiver");
            // Chromium accepts RTX for video, but rejects an audio RTX SSRC.
            transceiver.set_property("do-nack", pad.name() == "sink_0");
            transceiver.set_direction(rtc::WebRTCRTPTransceiverDirection::Sendonly);
        }
        let encoder = pipeline
            .by_name("encoder")
            .context("Missing encoder")?
            .downgrade();
        let pacer = gst::ElementFactory::make("rtpgccbwe")
            .property("min-bitrate", 1_000_000u32)
            .property("max-bitrate", 18_128_000u32)
            .property("estimated-bitrate", 18_128_000u32)
            .build()?;
        pacer.connect_notify(Some("estimated-bitrate"), move |pacer, _| {
            let bits = pacer.property::<u32>("estimated-bitrate");
            if let Some(encoder) = encoder.upgrade() {
                encoder.set_property("bitrate", (bits.saturating_sub(128_000) / 1000).max(500));
            }
        });
        transport.connect("request-aux-sender", false, move |_| Some(pacer.to_value()));
        transport.connect("on-ice-candidate", false, |args| {
            event(json!({"type":"candidate", "sdpMLineIndex":args[1].get::<u32>().unwrap(), "candidate":args[2].get::<String>().unwrap()}));
            None
        });
        let weak = transport.downgrade();
        transport.connect("on-negotiation-needed", false, move |_| {
            let Some(transport) = weak.upgrade() else {
                return None;
            };
            let weak = transport.downgrade();
            let promise = gst::Promise::with_change_func(move |reply| {
                let result = (|| -> Result<()> {
                    let reply = reply
                        .map_err(|e| anyhow::anyhow!("Offer failed: {e:?}"))?
                        .context("Empty offer")?;
                    let offer = reply.get::<rtc::WebRTCSessionDescription>("offer")?;
                    if let Some(transport) = weak.upgrade() {
                        transport.emit_by_name::<()>(
                            "set-local-description",
                            &[&offer, &None::<gst::Promise>],
                        );
                        event(json!({"type":"offer", "sdp":offer.sdp().as_text()?}));
                    }
                    Ok(())
                })();
                if let Err(e) = result {
                    event(json!({"type":"error", "message":e.to_string()}));
                }
            });
            transport.emit_by_name::<()>("create-offer", &[&None::<gst::Structure>, &promise]);
            None
        });
        transport.connect_notify(Some("connection-state"), |transport, _| {
            event(json!({"type":"connection", "state":format!("{:?}",transport.property::<rtc::WebRTCPeerConnectionState>("connection-state"))}));
        });
        let counters = Arc::new(Mutex::new(Counters::default()));
        for (name, capture) in [("capture", true), ("encoder", false)] {
            let count = counters.clone();
            pipeline
                .by_name(name)
                .context("Missing measured element")?
                .static_pad("src")
                .context("Missing measured pad")?
                .add_probe(gst::PadProbeType::BUFFER, move |_, _| {
                    let mut c = count.lock().unwrap();
                    if capture {
                        c.capture += 1;
                        let now = Instant::now();
                        if let Some(last) = c.last_capture {
                            c.gaps_ms
                                .push(now.duration_since(last).as_secs_f64() * 1000.0);
                        }
                        c.last_capture = Some(now);
                    } else {
                        c.encoded += 1;
                    }
                    gst::PadProbeReturn::Ok
                });
        }
        let mut last = Instant::now();
        let timer = gst::glib::timeout_add(std::time::Duration::from_secs(2), move || {
            let now = Instant::now();
            let seconds = now.duration_since(last).as_secs_f64();
            last = now;
            let mut c = counters.lock().unwrap();
            c.gaps_ms.sort_by(f64::total_cmp);
            let p95 = c
                .gaps_ms
                .get(c.gaps_ms.len().saturating_sub(1) * 95 / 100)
                .copied();
            event(
                json!({"type":"stats", "targetFps":60,"captureFps":(c.capture-c.previous_capture) as f64/seconds,
                "encodedFps":(c.encoded-c.previous_encoded) as f64/seconds,"captureGapP95Ms":p95,
                "captureFrames":c.capture,"encodedFrames":c.encoded,"encoder":if cfg!(target_os="windows"){"NVENC D3D11"}else{"VideoToolbox"}, "width":1920,"height":1080}),
            );
            c.previous_capture = c.capture;
            c.previous_encoded = c.encoded;
            c.gaps_ms.clear();
            gst::glib::ControlFlow::Continue
        });
        let bus = pipeline.bus().unwrap().add_watch(|_, message| {
            if let gst::MessageView::Error(e) = message.view() {
                event(json!({"type":"error", "message":e.error().to_string(), "detail":e.debug().map(|value| value.to_string())}));
            }
            gst::glib::ControlFlow::Continue
        })?;
        let mut sender = Self {
            #[cfg(target_os="macos")]
            capture: std::ptr::null_mut(),
            pipeline,
            transport,
            pending: vec![],
            answered: false,
            _bus: bus,
            timer: Some(timer),
            _timer_resolution: timer_resolution,
        };
        sender.pipeline.set_state(gst::State::Playing)?;
        #[cfg(target_os="macos")]
        { use gst::glib::translate::ToGlibPtr;
          let video=sender.pipeline.by_name("capture").unwrap();let audio=sender.pipeline.by_name("systemaudio");
          sender.capture=unsafe{pulse_capture_start(video.to_glib_none().0,audio.as_ref().map(|v|v.to_glib_none().0).unwrap_or(std::ptr::null_mut()))};
          if sender.capture.is_null(){bail!("Screen capture could not start. Allow Pulse Helper in macOS Screen Recording settings.");}
        }
        Ok(sender)
    }
    fn answer(&mut self, value: String) -> Result<()> {
        if value.len() > 65_000 || self.answered {
            bail!("Invalid or duplicate answer");
        }
        let description = rtc::WebRTCSessionDescription::new(
            rtc::WebRTCSDPType::Answer,
            sdp::SDPMessage::parse_buffer(value.as_bytes())?,
        );
        let promise = gst::Promise::new();
        self.transport
            .emit_by_name::<()>("set-remote-description", &[&description, &promise]);
        if promise.wait() != gst::PromiseResult::Replied {
            bail!("Remote answer was not applied");
        }
        if let Some(reply) = promise.get_reply() {
            if reply.has_field("error") {
                bail!("Remote answer rejected: {reply:?}");
            }
        }
        self.answered = true;
        for (line, candidate) in self.pending.drain(..) {
            self.transport
                .emit_by_name::<()>("add-ice-candidate", &[&line, &candidate]);
        }
        Ok(())
    }
    fn candidate(&mut self, line: u32, value: String) -> Result<()> {
        if line > 8 || value.len() > 4096 || self.pending.len() >= 128 {
            bail!("Invalid ICE candidate");
        }
        if self.answered {
            self.transport
                .emit_by_name::<()>("add-ice-candidate", &[&line, &value]);
        } else {
            self.pending.push((line, value));
        }
        Ok(())
    }
}
impl Drop for Sender {
    fn drop(&mut self) {
        #[cfg(target_os="macos")]
        unsafe{pulse_capture_stop(self.capture);}
        if let Some(timer) = self.timer.take() {
            timer.remove();
        }
        let _ = self.pipeline.set_state(gst::State::Null);
    }
}
#[cfg(target_os="macos")]
extern "C" {fn pulse_capture_start(video:*mut gst::ffi::GstElement,audio:*mut gst::ffi::GstElement)->*mut std::ffi::c_void;fn pulse_capture_stop(capture:*mut std::ffi::c_void);}
fn main() -> Result<()> {
    let Some(argument)=std::env::args().nth(1) else {return Ok(());};
    if let Ok(exe)=std::env::current_exe(){if let Some(root)=exe.parent(){let plugins=root.join("plugins");if plugins.exists(){std::env::set_var("GST_PLUGIN_SYSTEM_PATH",&plugins);std::env::set_var("GST_PLUGIN_PATH",&plugins);std::env::set_var("GST_REGISTRY_FORK","no");}}}
    if argument != "--media" {
        let result=helper::run(&argument);
        if let Err(ref error)=result {
            let root=std::env::var_os("LOCALAPPDATA").map(std::path::PathBuf::from).unwrap_or_else(std::env::temp_dir).join("Pulse Helper");
            let _=std::fs::create_dir_all(&root);
            let _=std::fs::write(root.join("last-launch-error.txt"),format!("Pulse Helper: {error:#}\n"));
        }
        return result;
    }
    gst::init()?;
    let main_loop = gst::glib::MainLoop::new(None, false);
    let run = main_loop.clone();
    std::thread::spawn(move || run.run());
    event(
        json!({"type":"ready","protocol":1,"version":env!("CARGO_PKG_VERSION"),"proofOnly":true}),
    );
    let mut sender: Option<Sender> = None;
    let mut input = io::stdin().lock();
    loop {
        // Bound input before allocating arbitrary data from even the trusted parent.
        let mut bytes = Vec::new();
        let count = std::io::Read::take(&mut input, 70_001).read_until(b'\n', &mut bytes)?;
        if count == 0 {
            break;
        }
        if count > 70_000 {
            bail!("Command exceeds protocol limit");
        }
        let result = (|| -> Result<()> {
            match serde_json::from_slice::<Command>(&bytes)? {
                Command::Start {
                    monitor,
                    audio,
                    wgc,
                } => {
                    if sender.is_some() {
                        bail!("Already sharing");
                    }
                    sender = Some(Sender::start(monitor, audio, wgc)?);
                }
                Command::Answer { sdp } => sender.as_mut().context("Not sharing")?.answer(sdp)?,
                Command::Candidate { line, candidate } => sender
                    .as_mut()
                    .context("Not sharing")?
                    .candidate(line, candidate)?,
                Command::Stop => {
                    sender = None;
                    event(json!({"type":"stopped"}));
                }
            }
            Ok(())
        })();
        if let Err(e) = result {
            event(json!({"type":"error","message":e.to_string()}));
        }
    }
    drop(sender);
    main_loop.quit();
    Ok(())
}
