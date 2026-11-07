// Good to know: the questions people ask first.

const faq = [
  {
    q: 'Do I need Docker?',
    a: 'No. Sawit uses PostgreSQL from Homebrew, Postgres.app or your PATH, or downloads PostgreSQL 16, 17 or 18 itself.',
  },
  {
    q: 'Which Macs does it run on?',
    a: 'Apple Silicon, macOS only. Sawit leans on the Keychain, lsof, AVPlayer and CoreGraphics.',
  },
  {
    q: 'Why does macOS say it cannot verify Sawit?',
    a: 'Sawit is not notarized yet. The install script avoids the warning. With the DMG, open Sawit once and choose Done, then Open Anyway in System Settings → Privacy & Security, or run xattr -dr com.apple.quarantine /Applications/Sawit.app.',
  },
  {
    q: 'Where do my passwords go?',
    a: 'Into the macOS Keychain, never a file. The table browser reads over a separate read-only connection.',
  },
  {
    q: 'How does it update?',
    a: 'After the first install the app updates itself. To install a given release instead of the latest, put SAWIT_VERSION=vX.Y.Z (a tag from the releases page) before sh.',
  },
  {
    q: 'How much memory does it use?',
    a: 'About 115 MB idle with one window and a terminal. Only the browser tab on show keeps a live page, so a busy session gives memory back instead of piling it up.',
  },
  {
    q: 'I use Claude Pool. What now?',
    a: 'Agent Accounts on the Accounts page takes over. Import from Claude Pool copies your accounts, settings and usage history, and Sawit offers to remove Claude Pool once nothing needs it. Sign-ins are never touched.',
  },
  {
    q: 'Can I edit a single cell in the grid?',
    a: 'Not yet. The grid deletes rows or sets one column on the rows you select; single cells go through the SQL editor.',
  },
]

export function Faq() {
  return (
    <section className="sec" id="faq" aria-labelledby="faq-title">
      <div className="w pad">
        <h2 id="faq-title" className="h2 c">
          Good to know
        </h2>
        <div className="faq">
          {faq.map((item) => (
            <div key={item.q}>
              <h3>{item.q}</h3>
              <p>{item.a}</p>
            </div>
          ))}
        </div>
      </div>
    </section>
  )
}
