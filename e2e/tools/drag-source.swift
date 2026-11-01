// A file to drag, in a small window of another app, as Finder would offer one. Sawit's terminal
// types a dropped file's quoted path, and that has to come from a real drag between apps:
// AppKit starts one (an NSDraggingSession) only from a real mouse drag in the source app.
//
// Usage: drag-source <file> <x> <y> [<to-x> <to-y>]
//   Shows the source, a 120 pt square, centred on (x, y) in global top-left coordinates (the
//   CGEvent space agent-desktop uses). With a target, it drives the drag itself with HID mouse
//   events, from the source to the target. Without one, it waits for a drag from outside (an
//   `agent-desktop --headed drag`). It prints "ended <operation>" when the drag is over and
//   exits, or exits 2 after 20 s with no drag.
//
// Either way the real pointer moves: run it on an idle Mac only.
import AppKit
import CoreGraphics

let args = CommandLine.arguments
guard args.count == 4 || args.count == 6 else {
    FileHandle.standardError.write("usage: drag-source <file> <x> <y> [<to-x> <to-y>]\n".data(using: .utf8)!)
    exit(64)
}
let file = URL(fileURLWithPath: args[1])
let source = CGPoint(x: Double(args[2])!, y: Double(args[3])!)
let target = args.count == 6 ? CGPoint(x: Double(args[4])!, y: Double(args[5])!) : nil

final class Source: NSView, NSDraggingSource {
    override func draw(_ dirtyRect: NSRect) {
        NSColor.systemOrange.setFill()
        dirtyRect.fill()
    }
    override func mouseDown(with event: NSEvent) {}
    override func mouseDragged(with event: NSEvent) {
        let item = NSDraggingItem(pasteboardWriter: file as NSURL)
        item.setDraggingFrame(bounds, contents: NSImage(named: NSImage.multipleDocumentsName))
        beginDraggingSession(with: [item], event: event, source: self)
    }
    func draggingSession(_ session: NSDraggingSession, sourceOperationMaskFor context: NSDraggingContext) -> NSDragOperation {
        .copy
    }
    func draggingSession(_ session: NSDraggingSession, endedAt screenPoint: NSPoint, operation: NSDragOperation) {
        print("ended \(operation.rawValue)")
        fflush(stdout)
        DispatchQueue.main.asyncAfter(deadline: .now() + 0.3) { NSApp.terminate(nil) }
    }
}

func post(_ type: CGEventType, _ at: CGPoint) {
    CGEvent(mouseEventSource: nil, mouseType: type, mouseCursorPosition: at, mouseButton: .left)!
        .post(tap: .cghidEventTap)
}

let app = NSApplication.shared
app.setActivationPolicy(.accessory)
let screenHeight = NSScreen.screens[0].frame.height
let frame = NSRect(x: source.x - 60, y: screenHeight - source.y - 60, width: 120, height: 120)
let window = NSWindow(contentRect: frame, styleMask: [.borderless], backing: .buffered, defer: false)
window.level = .floating
window.contentView = Source(frame: NSRect(origin: .zero, size: frame.size))
window.orderFrontRegardless()
print("ready")
fflush(stdout)

DispatchQueue.main.asyncAfter(deadline: .now() + 20) {
    FileHandle.standardError.write("no drag within 20 s\n".data(using: .utf8)!)
    exit(2)
}

if let target {
    DispatchQueue.global().async {
        usleep(400_000)
        post(.mouseMoved, source)
        usleep(200_000)
        post(.leftMouseDown, source)
        usleep(150_000)
        let steps = 40
        for i in 1...steps {
            let t = Double(i) / Double(steps)
            post(.leftMouseDragged, CGPoint(x: source.x + (target.x - source.x) * t,
                                            y: source.y + (target.y - source.y) * t))
            usleep(15_000)
        }
        // Resting on the target first, as a hand does: AppKit hands the drop to the view under
        // the pointer once the drag has entered it.
        usleep(400_000)
        post(.leftMouseUp, target)
    }
}
app.run()
