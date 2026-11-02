// Sawit's control protocol (crates/core/src/app_control/): one JSON command per connection on
// the app's Unix socket. The client writes the magic line and the request, closes its sending
// half, and reads one reply line, `ok <json>` or `error: <message>`.
//
// The tests use it for setup and for reading state back (`state`, `terminal_read`), never in place
// of the UI action under test.

import net from "node:net";

const MAGIC = "sawit-control/1\n";

export class ControlError extends Error {}

/** Sends `request` (`{ cmd, ... }`) to the socket at `path`; resolves with the reply's JSON. */
export function send(path, request, { timeoutMs = 10_000 } = {}) {
  return new Promise((resolve, reject) => {
    const socket = net.createConnection(path);
    let reply = "";
    const timer = setTimeout(() => {
      socket.destroy();
      reject(new ControlError(`no reply to ${request.cmd} within ${timeoutMs} ms`));
    }, timeoutMs);
    socket.setEncoding("utf8");
    socket.on("connect", () => socket.end(MAGIC + JSON.stringify(request)));
    socket.on("data", (chunk) => (reply += chunk));
    socket.on("error", (e) => {
      clearTimeout(timer);
      reject(e);
    });
    socket.on("close", () => {
      clearTimeout(timer);
      const line = reply.split("\n")[0];
      if (line.startsWith("ok ")) {
        try {
          resolve(JSON.parse(line.slice(3)));
        } catch (e) {
          reject(new ControlError(`bad reply to ${request.cmd}: ${line}`));
        }
      } else if (line.startsWith("error: ")) {
        reject(new ControlError(line.slice(7).replaceAll("\\n", "\n").replaceAll("\\\\", "\\")));
      } else {
        reject(new ControlError(`unexpected reply to ${request.cmd}: ${JSON.stringify(reply)}`));
      }
    });
  });
}
