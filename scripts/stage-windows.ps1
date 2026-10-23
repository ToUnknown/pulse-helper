param([string]$Runtime,[string]$Binary,[string]$Output)
$ErrorActionPreference='Stop'
New-Item -ItemType Directory -Force "$Output\plugins" | Out-Null
Copy-Item $Binary "$Output\pulse-media.exe" -Force
# Ship only media plugins actually used by the capture pipeline.
'coreelements','typefindfunctions','d3d11','nvcodec','audioconvert','audioresample','opus','rtp','rtpmanager','webrtc','nice','dtls','srtp','sctp','videoparsersbad','wasapi2','rsrtp' | ForEach-Object {Copy-Item "$Runtime\lib\gstreamer-1.0\gst$_.dll" "$Output\plugins" -Force}
# Runtime libraries are distributed beside the executable; no machine-wide install.
Copy-Item "$Runtime\bin\*.dll" $Output -Force
Copy-Item "$Runtime\share\licenses" "$Output\licenses" -Recurse -Force -ErrorAction SilentlyContinue
