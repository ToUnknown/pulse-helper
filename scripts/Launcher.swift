import AppKit
final class Delegate: NSObject, NSApplicationDelegate {
    var child: Process?
    func application(_ application: NSApplication, open urls: [URL]) {
        guard child == nil, let url=urls.first, url.scheme == "pulse-capture" else{return}
        let process=Process();process.executableURL=Bundle.main.executableURL!.deletingLastPathComponent().appendingPathComponent("pulse-media");process.arguments=[url.absoluteString]
        process.standardOutput=FileHandle.nullDevice;process.standardError=FileHandle.nullDevice
        process.terminationHandler={_ in DispatchQueue.main.async {NSApplication.shared.terminate(nil)}}
        do{try process.run();child=process}catch{NSApplication.shared.terminate(nil)}
    }
    func applicationDidFinishLaunching(_ notification: Notification) {
        DispatchQueue.main.asyncAfter(deadline:.now()+3){if self.child==nil{NSApplication.shared.terminate(nil)}}
    }
    func applicationWillTerminate(_ notification:Notification){child?.terminate()}
}
let application=NSApplication.shared
let delegate=Delegate();application.delegate=delegate;application.setActivationPolicy(.accessory);application.run()
