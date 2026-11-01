export const REPO = 'https://github.com/bahrulbangsawan/sawit.dev'
// Every release attaches Sawit.dmg under the same name, so this always fetches the newest one.
export const DMG = `${REPO}/releases/latest/download/Sawit.dmg`
export const INSTALL = 'curl -fsSL https://sawit.dev/install.sh | sh'
// The release binary, as README's Size & memory measured it: change both together.
export const SIZE = '60 MiB'
// The site's address, for the absolute URLs link previews and canonical links need.
export const SITE = 'https://sawit.dev'
export const DESCRIPTION = 'One native app for local development: Postgres without Docker, an editor, terminals, a browser, a canvas and dev tools, ready for AI agents.'
