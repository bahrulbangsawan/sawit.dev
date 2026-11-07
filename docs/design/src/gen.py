# Builds the Accounts page design screens (HTML on the Sawit canvas) from shared parts.
# Sample data only: names, emails and figures are made up.
import os, sys, shutil
sys.path.insert(0, os.path.dirname(__file__))
from icons import i

ROOT = os.path.join(os.path.dirname(__file__), "..", "accounts.screens")
CSS = open(os.path.join(os.path.dirname(__file__), "..", "sawit.css")).read()

NAV = [
    ("Accounts", [("users", "Accounts", None), ("chart", "Usage", None)]),
    ("Claude Code", [("term", "Status Line", None)]),
    ("Setup", [("steth", "Diagnostics", "1"), ("gear", "Settings", None)]),
]

def nav(sel, icons=False, groups=NAV):
    out = [f'<nav class="nav{" icons" if icons else ""}" role="tablist" aria-orientation="vertical">']
    for gi, (g, tabs) in enumerate(groups):
        if icons and gi: out.append('<div class="sep"></div>')
        out.append(f'<div class="group">{g}</div>')
        for ic, label, count in tabs:
            s = " sel" if label == sel else ""
            c = f'<span class="count num">{count}</span>' if count and not icons else ""
            text = "" if icons else f"<span>{label}</span>"
            out.append(f'<div class="tab{s}" role="tab" aria-selected="{str(bool(s)).lower()}" title="{label}">{i(ic)}{text}{c}</div>')
    out.append("</nav>")
    return "".join(out)

def head(summary, actions=""):
    return f'''<header class="head">{i("users","i i16",'color:var(--muted)')}<h1>Accounts</h1>
<span class="muted sm truncate num">{summary}</span><div class="grow"></div>{actions}</header>'''

def page(body, sel, summary="3 accounts · 1 active · next reset in 2h 14m", icons=False, banner="", groups=NAV):
    return f'''<!doctype html><html><head><meta charset="utf-8"><link rel="stylesheet" href="sawit.css"><!--rev--></head>
<body><div class="page">{head(summary)}<div class="body">{nav(sel, icons, groups)}
<main class="grow col" style="min-height:0">{banner}{body}</main></div></div></body></html>'''

ACCOUNTS = [
    # name, cp, email, avatar class, 5h, 7d, active, state
    ("work", 1, "dana@studio-lumen.com", "a-orange", 23, 41, True, None),
    ("personal", 2, "dana.k@fastmail.com", "a-blue", 72, 88, False, None),
    ("client", 3, "dana@harbor-labs.io", "a-green", 100, 64, False, "limit"),
    ("spare", 4, "", "a-violet", None, None, False, "signed_out"),
]

def lvl(p):
    return "" if p is None or p < 70 else ("warn" if p < 90 else "bad")

def mini(label, p):
    if p is None:
        return f'<div class="col grow" style="gap:4px"><span class="xs muted">{label} · no data</span><div class="track"></div></div>'
    return f'''<div class="col grow" style="gap:4px"><span class="xs muted num">{label} {p}% used</span>
<div class="track"><div class="fill {lvl(p)}" style="width:{p}%"></div></div></div>'''

def row(a, sel=False, compact=False):
    name, cp, email, av, h5, d7, active, state = a
    status = ""
    if active: status = f'<span class="pill ok">{i("check")}Active</span>'
    elif state == "limit": status = f'<span class="pill bad">{i("x")}At limit</span>'
    elif state == "signed_out": status = f'<span class="pill">{i("login")}Signed out</span>'
    bg = "background:var(--row-active);" if sel else ""
    ring = "box-shadow: inset 0 0 0 2px var(--ring);" if sel else ""
    who = email or "~/.claude-" + name
    meters = "" if compact else f'<div class="row" style="gap:12px;margin-top:6px">{mini("5h", h5)}{mini("7d", d7)}</div>'
    return f'''<div role="listitem" aria-label="{name}" style="display:flex;gap:10px;align-items:flex-start;padding:8px 10px;border-radius:6px;{bg}{ring}">
<div class="avatar {av}">{name[0].upper()}</div>
<div class="col grow"><div class="row" style="gap:6px"><span class="semi truncate">{name}</span><span class="badge">cp{cp}</span><div class="grow"></div>{status}</div>
<span class="xs muted truncate">{who}</span>{meters}</div></div>'''

def account_list(sel=0, width=300, compact=False):
    rows = "".join(row(a, ix == sel, compact) for ix, a in enumerate(ACCOUNTS))
    return f'''<section class="col" style="width:{width}px;flex:none;border-right:1px solid var(--border);min-height:0">
<div style="padding:12px 12px 8px"><button class="btn" style="width:100%">{i("plus")}Add Account</button></div>
<div class="row" style="padding:8px 14px 4px;gap:6px"><span style="width:14px;height:14px;border-radius:3px;background:#d97757;flex:none"></span><span class="xs semi muted">Claude Code</span><span class="xs muted num" style="margin-left:auto">4</span></div>
<div class="col" role="list" style="padding:0 6px 12px;gap:2px;overflow:hidden">{rows}</div></section>'''

def meter_card(title, p, resets, at):
    icon = ""
    if lvl(p) == "warn": icon = i("warn", "i", "color:var(--warning)")
    if lvl(p) == "bad": icon = i("x", "i", "color:var(--danger)")
    return f'''<div class="box" style="flex:1;min-width:0;padding:14px 16px">
<div class="row"><span class="sm muted">{title}</span><div class="grow"></div>{icon}</div>
<div class="row" style="align-items:baseline;gap:6px;margin:6px 0 10px"><span class="num" style="font-size:28px;line-height:32px;font-weight:500">{p}%</span><span class="sm muted">used</span></div>
<div class="track" style="height:6px;border-radius:3px"><div class="fill {lvl(p)}" style="width:{p}%;border-radius:3px"></div></div>
<div class="row xs muted num" style="margin-top:8px"><span>Resets in {resets}</span><div class="grow"></div><span>{at}</span></div></div>'''

def detail(narrow=False):
    gap = "24px"
    sessions = f'''<div class="row" style="padding:10px 16px;border-top:1px solid var(--border)">{i("term","i",'color:var(--muted)')}<span class="sm grow truncate">sawit.dev</span><span class="xs muted">Working</span><button class="btn xs">Continue On…{i("chev")}</button></div>
<div class="row" style="padding:10px 16px;border-top:1px solid var(--border)">{i("term","i",'color:var(--muted)')}<span class="sm grow truncate">harbor-site</span><span class="xs muted">Waiting for you</span><button class="btn xs">Continue On…{i("chev")}</button></div>'''
    return f'''<section class="grow" style="overflow:hidden;padding:24px">
<div class="col" style="max-width:720px;gap:{gap}">
<div class="row" style="gap:12px;align-items:center">
<div class="avatar l a-orange">W</div>
<div class="col grow"><div class="row" style="gap:8px"><span style="font-size:18px;line-height:24px;font-weight:600">work</span><span class="badge">cp1</span><span class="pill">Max 20x</span></div>
<span class="sm muted truncate">dana@studio-lumen.com</span></div>
<span class="pill ok">{i("check")}Active</span>
<button class="btn primary">{i("term")}Open in Terminal</button>
<button class="btn icon ghost" aria-label="More actions for work">{i("dots")}</button></div>

<div><div class="section-h">{i("gauge")}<h2>Usage limits</h2><span class="xs muted">Updated 2 min ago</span><div class="grow"></div><button class="btn ghost xs">{i("refresh")}Check Usage</button></div>
<div class="row" style="gap:12px;align-items:stretch{';flex-direction:column' if narrow else ''}">{meter_card("5-hour window", 23, "2h 14m", "16:30")}{meter_card("7-day window", 41, "3d 4h", "Mon 09:00")}</div></div>

<div><div class="section-h">{i("folder")}<h2>Details</h2></div>
<div class="box">
<div class="row" style="padding:10px 16px"><span class="sm muted" style="width:140px;flex:none">Folder</span><span class="sm grow truncate" style="font-family:ui-monospace,Menlo;font-size:12px">~/.claude-work</span><button class="btn ghost xs">{i("folder")}Reveal</button><button class="btn ghost xs">{i("copy")}Copy Path</button></div>
<div class="row" style="padding:10px 16px;border-top:1px solid var(--border)"><span class="sm muted" style="width:140px;flex:none">Runs with</span><span class="grow truncate" style="font-family:ui-monospace,Menlo;font-size:12px">CLAUDE_CONFIG_DIR=~/.claude-work</span></div>
<div class="row" style="padding:10px 16px;border-top:1px solid var(--border)"><span class="sm muted" style="width:140px;flex:none">Sessions</span><span class="sm grow">2 running as this account</span></div>
{sessions}
</div></div>
</div></section>'''

def write(name, html):
    d = os.path.join(ROOT, name)
    os.makedirs(d, exist_ok=True)
    open(os.path.join(d, "index.html"), "w").write(html)
    open(os.path.join(d, "sawit.css"), "w").write(CSS)

SCREENS = {}

SCREENS["accounts-desktop"] = page(f'<div class="row" style="flex:1;min-height:0;align-items:stretch;gap:0">{account_list()}{detail()}</div>', "Accounts")
SCREENS["accounts-narrow-2"] = page(f'<div class="row" style="flex:1;min-height:0;align-items:stretch;gap:0">{account_list(width=248, compact=True)}{detail(narrow=True)}</div>', "Accounts", summary="3 accounts · 1 active", icons=True)

empty = f'''<div class="empty" style="margin:auto;max-width:420px">
<div class="ic">{i("users","i i16")}</div><div style="font-size:16px;line-height:24px;font-weight:600">No Claude Code accounts yet</div>
<div class="sm muted">An account is a Claude Code folder with its own sign-in. Add one to sign in, or adopt a ~/.claude-* folder you already have.</div>
<div class="row" style="margin-top:8px"><button class="btn primary">{i("plus")}Add Account</button><button class="btn">{i("folder")}Adopt Existing…</button></div></div>'''
SCREENS["accounts-empty-2"] = page(empty, "Accounts", summary="No accounts")

def setting(ic, title, desc, control, dim=False):
    o = "opacity:.55;" if dim else ""
    return f'''<div class="setting"><div class="ic">{i(ic)}</div><div class="col grow" style="{o}"><span class="sm" style="font-weight:500">{title}</span><span class="xs muted">{desc}</span></div>{control}</div>'''

def group(ic, title, rows, note=""):
    return f'''<div><div class="section-h">{i(ic)}<h2>{title}</h2></div><div class="box">{"".join(rows)}</div>{note}</div>'''

settings = f'''<div style="overflow:hidden;padding:24px"><div class="col" style="max-width:720px;gap:24px">
{group("term", "Shell", [
  setting("term", "Start claude as the active account", "Adds a block to ~/.zshrc so a plain claude in a new terminal uses the active account.", f'<span class="pill ok">{i("check")}Installed</span><button class="btn xs ghost danger">Remove…</button>'),
  setting("key", "Shortcuts cp1–cp9", "cp2 starts claude as the second account, in the terminal you type it in.", f'<button class="btn xs">{i("plus")}Install</button>'),
])}
{group("share", "Shared settings", [
  setting("share", "Share settings between accounts", "MCP servers, skills, agents, commands, CLAUDE.md, plugins, hooks and permissions, from ~/.claude. Sign-ins stay per account.", '<span class="switch on" role="switch" aria-checked="true"></span>'),
  setting("refresh", "Sync now", "Last run 12 min ago: everything was already the same.", f'<button class="btn xs">{i("refresh")}Sync Now</button>'),
])}
{group("bell", "Limit alerts", [
  setting("bell", "Notify near and at a limit", "Alerts only inform: nothing switches accounts by itself.", '<span class="switch" role="switch" aria-checked="false"></span>'),
  setting("gauge", "Alert threshold", "Turn on alerts to choose when they fire.", '<span class="seg dis" aria-disabled="true"><span>75%</span><span>80%</span><span class="on">85%</span><span>90%</span><span>95%</span></span>', dim=True),
  setting("users", "Suggest the account with the most room", "Shows a switch suggestion in the account chip when the active one is near its limit.", '<span class="switch off-dis" role="switch" aria-checked="false" aria-disabled="true"></span>', dim=True),
])}
{group("power", "Startup", [
  setting("power", "Open Sawit at login", "Keeps shared settings in sync and the title bar's usage current.", '<span class="switch on" role="switch" aria-checked="true"></span>'),
])}
</div></div>'''
SCREENS["settings-desktop"] = page(settings, "Settings")

def check(level, title, detail, fix=None):
    ic, color = {"pass": ("ok", "var(--success)"), "warn": ("warn", "var(--warning)"), "fail": ("x", "var(--danger)"), "info": ("info", "var(--muted)")}[level]
    f = f'<button class="btn xs">{fix}</button>' if fix else ""
    return f'''<div class="row" style="padding:10px 16px;align-items:flex-start;gap:12px">{i(ic,"i i16",f"color:{color};margin-top:2px")}<div class="col grow"><span class="sm" style="font-weight:500">{title}</span><span class="xs muted">{detail}</span></div>{f}</div>'''

checks = [
  check("pass", "Claude Code is installed", "claude 2.1.32 at /opt/homebrew/bin/claude"),
  check("pass", "Every account is signed in", "work, personal and client answered as signed in"),
  check("warn", "spare is signed out", "New sessions as spare will ask to sign in.", "Sign In…"),
  check("pass", "Usage tracking runs", "3 of 4 accounts report their limits through the status line"),
  check("pass", "Shell block is current", "~/.zshrc starts claude as the active account"),
  check("info", "Shortcuts aren't installed", "cp1–cp9 aren't on PATH; Open in Terminal works without them.", "Install"),
]
diag = f'''<div style="overflow:hidden;padding:24px"><div class="col" style="max-width:720px;gap:16px">
<div class="row"><span class="pill ok">{i("ok")}4 passed</span><span class="pill warn">{i("warn")}1 warning</span><span class="pill">{i("info")}1 note</span><div class="grow"></div><button class="btn xs">{i("refresh")}Run Again</button><button class="btn xs">{i("copy")}Copy Report</button></div>
<div class="box" style="padding:4px 0">{"".join(c if ix == 0 else c.replace('class="row" style="padding', 'class="row" style="border-top:1px solid var(--border);padding', 1) for ix, c in enumerate(checks))}</div>
</div></div>'''
SCREENS["diagnostics-desktop"] = page(diag, "Diagnostics")

def tile(label, value, change):
    return f'''<div class="box" style="flex:1;padding:12px 16px"><span class="xs muted">{label}</span><div class="num" style="font-size:24px;line-height:32px;font-weight:500">{value}</div><span class="xs muted num">{change}</span></div>'''

usage = f'''<div style="overflow:hidden;padding:24px"><div class="col" style="max-width:960px;gap:24px">
<div class="row"><span class="seg"><span>24 hours</span><span class="on">7 days</span><span>30 days</span></span><div class="grow"></div><span class="xs muted">Tokens Claude Code recorded on this Mac. Plan limits are on the Accounts tab.</span></div>
<div class="row" style="gap:12px">{tile("Tokens", "48.2M", "+12% vs the 7 days before")}{tile("Requests", "3,914", "+4% vs the 7 days before")}{tile("Cache hit rate", "91%", "−2 points vs the 7 days before")}</div>
<div><div class="section-h">{i("chart")}<h2>By account and model</h2></div><div class="box"><table>
<tr><th>Account</th><th>Model</th><th class="r">Input</th><th class="r">Output</th><th class="r">Cache read</th><th class="r">Requests</th></tr>
<tr><td><div class="row"><div class="avatar a-orange" style="width:20px;height:20px;font-size:10px">W</div>work</div></td><td>opus-4.1</td><td class="r">1.2M</td><td class="r">640k</td><td class="r">31.0M</td><td class="r">2,140</td></tr>
<tr><td><div class="row"><div class="avatar a-orange" style="width:20px;height:20px;font-size:10px">W</div>work</div></td><td>sonnet-4.5</td><td class="r">410k</td><td class="r">220k</td><td class="r">8.4M</td><td class="r">980</td></tr>
<tr><td><div class="row"><div class="avatar a-blue" style="width:20px;height:20px;font-size:10px">P</div>personal</div></td><td>sonnet-4.5</td><td class="r">180k</td><td class="r">96k</td><td class="r">5.9M</td><td class="r">794</td></tr>
</table></div></div>
<div><div class="section-h">{i("term")}<h2>Running sessions</h2><span class="xs muted num">2</span></div><div class="box">
<div class="row" style="padding:10px 16px"><div class="avatar a-orange" style="width:20px;height:20px;font-size:10px">W</div><span class="sm grow">sawit.dev</span><span class="xs muted">Working · pid 48122</span><button class="btn xs">Continue On…{i("chev")}</button><button class="btn xs ghost danger">End…</button></div>
<div class="row" style="padding:10px 16px;border-top:1px solid var(--border)"><div class="avatar a-orange" style="width:20px;height:20px;font-size:10px">W</div><span class="sm grow">harbor-site</span><span class="xs muted">Waiting for you · pid 50331</span><button class="btn xs">Continue On…{i("chev")}</button><button class="btn xs ghost danger">End…</button></div>
</div></div></div></div>'''
SCREENS["usage-desktop"] = page(usage, "Usage")

if __name__ == "__main__":
    only = sys.argv[1:]
    for name, html in SCREENS.items():
        if not only or name in only:
            write(name, html)
    print("wrote", ", ".join(only or SCREENS))

# ---- C1: the pool views, added to the same page --------------------------------------------
NAV_C1 = [
    ("Pool", [("dash", "Overview", None), ("list", "Detailed", None), ("compare", "Compare", None),
              ("cal", "Timeline", None), ("trend", "Analytics", None), ("wallet", "Wallet", None)]),
    ("Accounts", [("users", "Accounts", None), ("chart", "Usage", None)]),
    ("Claude Code", [("term", "Status Line", None)]),
    ("Setup", [("steth", "Diagnostics", "1"), ("gear", "Settings", None)]),
]
POOL = [
    # name, provider, avatar, plan, 5h used, 5h reset, 7d used, 7d reset, status, tone
    ("work", "Claude Code", "a-orange", "Max 20x", 23, "2h 14m", 41, "3d 4h", "Plenty left", "ok"),
    ("personal", "Claude Code", "a-blue", "Pro", 72, "3h 30m", 88, "1d 6h", "Running low", "warn"),
    ("client", "Claude Code", "a-green", "Max 5x", 100, "36m", 64, "5d", "Limit reached", "bad"),
    ("codex", "Codex", "a-violet", "Plus", 12, "4h 10m", 30, "6d 2h", "Plenty left", "ok"),
]
STATUS_ICON = {"ok": "ok", "warn": "warn", "bad": "x", "": "info"}

def bar(p, w=None):
    st = f"width:{w}px;flex:none" if w else ""
    return f'<div class="track" style="height:6px;{st}"><div class="fill {lvl(p)}" style="width:{p}%"></div></div>'

def limit_cell(p, reset):
    return f'''<div class="col" style="gap:4px;width:150px;flex:none"><div class="row xs num"><span>{p}% used</span><div class="grow"></div><span class="muted">{reset}</span></div>{bar(p)}</div>'''

def pool_row(a, compact=False):
    name, prov, av, plan, h5, r5, d7, r7, status, tone = a
    return f'''<div class="row" style="padding:10px 16px;gap:12px">
<div class="avatar {av}">{name[0].upper()}</div>
<div class="col" style="width:150px;flex:none"><span class="semi">{name}</span><span class="xs muted">{prov} · {plan}</span></div>
{limit_cell(h5, r5)}{limit_cell(d7, r7)}<div class="grow"></div>
<span class="pill {tone}">{i(STATUS_ICON[tone])}{status}</span></div>'''

def rows_box(items):
    out = []
    for ix, it in enumerate(items):
        out.append(it if ix == 0 else it.replace('<div class="row" style="padding', '<div class="row" style="border-top:1px solid var(--border);padding', 1))
    return f'<div class="box">{"".join(out)}</div>'

def c1_page(body, sel, actions=""):
    return page(f'<div style="overflow:hidden;padding:24px"><div class="col" style="max-width:1040px;gap:24px">{body}</div></div>', sel, summary="4 accounts · 2 agents · next reset in 36m", groups=NAV_C1)

overview = f'''
<div class="row"><div class="box row" style="flex:1;padding:14px 16px;gap:14px;border-color:rgba(116,173,232,.35)">
<div class="avatar l a-violet">C</div><div class="col grow"><span class="xs muted">Most room right now</span><span style="font-size:16px;font-weight:600">codex · 70% of its weekly limit left</span><span class="xs muted">Next best: work, 59% left · client is at its 5-hour limit until 14:36</span></div>
<button class="btn">{i("compare")}Compare</button></div></div>
<div><div class="section-h">{i("dash")}<h2>Accounts</h2><span class="xs muted">Updated 1 min ago</span><div class="grow"></div><button class="btn xs ghost">{i("refresh")}Refresh</button></div>
{rows_box([pool_row(a) for a in POOL])}</div>
<div class="row" style="gap:16px;align-items:flex-start">
<div style="flex:1"><div class="section-h">{i("bot")}<h2>Agents in Sawit</h2><span class="xs muted">2</span></div>
{rows_box([
 f'<div class="row" style="padding:10px 16px">{i("loader","i",'color:var(--fg)')}<span class="sm grow">Claude Code · sawit.dev</span><span class="xs muted">Terminal 2 · as work</span><span class="pill">{i("loader")}Working</span></div>',
 f'<div class="row" style="padding:10px 16px">{i("bell","i",'color:var(--warning)')}<span class="sm grow">Codex · harbor-site</span><span class="xs muted">Terminal 3 · as codex</span><span class="pill warn">{i("bell")}Needs you</span></div>',
 f'<div class="row" style="padding:10px 16px">{i("bot","i",'color:var(--muted)')}<span class="sm grow">Claude Code (MCP)</span><span class="xs muted">Canvas · 2 min ago</span><span class="pill">Idle</span></div>'])}</div>
<div style="width:380px;flex:none"><div class="section-h">{i("bell")}<h2>Notifications</h2><span class="badge">2 new</span><div class="grow"></div><button class="btn xs ghost">{i("check")}Mark All as Read</button></div>
{rows_box([
 f'<div class="row" style="padding:10px 16px">{i("x","i",'color:var(--danger)')}<span class="sm grow">client reached its limit</span><span class="xs muted">13:51</span></div>',
 f'<div class="row" style="padding:10px 16px">{i("warn","i",'color:var(--warning)')}<span class="sm grow">personal is running low</span><span class="xs muted">12:10</span></div>',
 f'<div class="row" style="padding:10px 16px;opacity:.7">{i("ok","i",'color:var(--success)')}<span class="sm grow">work has room again</span><span class="xs muted">Yesterday</span></div>'])}</div></div>'''
SCREENS["c1-overview-2"] = c1_page(overview, "Overview")

detailed = f'''<div class="row" style="flex-wrap:wrap"><span class="seg"><span class="on">By provider</span><span>By urgency</span></span>
<button class="btn xs">{i("list")}All providers{i("chev")}</button><div class="grow"></div><span class="xs muted">Show limits as</span><span class="seg"><span class="on">% used</span><span>% left</span></span></div>
<div><div class="section-h"><span style="width:14px;height:14px;border-radius:3px;background:#d97757"></span><h2>Claude Code</h2><span class="xs muted">3</span><div class="grow"></div><button class="btn xs ghost icon" aria-label="Move Claude Code down">{i("chev")}</button></div>
{rows_box([pool_row(a) for a in POOL[:3]])}</div>
<div><div class="section-h"><span style="width:14px;height:14px;border-radius:3px;background:#b48edc"></span><h2>Codex</h2><span class="xs muted">1</span></div>{rows_box([pool_row(POOL[3])])}</div>'''
SCREENS["c1-detailed-2"] = c1_page(detailed, "Detailed")

compare = f'''<div class="box" style="padding:20px;display:flex;gap:16px;align-items:center">
<div class="avatar l a-violet">C</div><div class="col grow"><span class="xs muted">Recommended right now</span>
<span style="font-size:20px;line-height:28px;font-weight:600">codex</span><span class="sm">70% of its weekly limit left, the most of any account</span>
<span class="xs muted">Held back by: weekly limit · resets Mon 09:00</span></div>
<button class="btn primary">{i("term")}Open in Terminal</button></div>
<div class="box"><table><tr><th>Account</th><th>Room left</th><th>Held back by</th><th class="r">Resets</th><th>Status</th></tr>
<tr><td>work</td><td><div class="row">{bar(59,120)}<span class="num xs">59%</span></div></td><td>Weekly</td><td class="r">Mon 09:00</td><td><span class="pill ok">{i("ok")}Plenty left</span></td></tr>
<tr><td>personal</td><td><div class="row">{bar(12,120)}<span class="num xs">12%</span></div></td><td>Weekly</td><td class="r">Sat 20:00</td><td><span class="pill warn">{i("warn")}Running low</span></td></tr>
<tr><td>client</td><td><div class="row">{bar(0,120)}<span class="num xs">0%</span></div></td><td>5-hour</td><td class="r">14:36</td><td><span class="pill bad">{i("x")}Limit reached</span></td></tr></table></div>
<div class="row xs muted" style="gap:6px">{i("info")}Room is a share of each plan: 30% of a Max 20x plan does more work than 30% of Pro.</div>'''
SCREENS["c1-compare-2"] = c1_page(compare, "Compare")

def tl_row(name, av, segs, now=46):
    s = "".join(f'<div style="position:absolute;left:{a}%;width:{w}%;top:6px;height:16px;border-radius:4px;background:{c}"></div>' for a, w, c in segs)
    return f'''<div class="row" style="padding:8px 16px;gap:12px"><div class="avatar {av}" style="width:22px;height:22px;font-size:10px">{name[0].upper()}</div><span class="sm" style="width:90px">{name}</span>
<div style="position:relative;flex:1;height:28px">{s}<div style="position:absolute;left:{now}%;top:0;bottom:0;width:2px;background:var(--fg)"></div></div></div>'''
timeline = f'''<div class="row"><span class="seg"><span class="on">Next 24 hours</span><span>This week</span></span><div class="grow"></div>
<span class="xs muted row" style="gap:12px"><span class="row" style="gap:4px"><span style="width:10px;height:10px;border-radius:2px;background:rgba(116,173,232,.55)"></span>Current window</span><span class="row" style="gap:4px"><span style="width:10px;height:10px;border-radius:2px;border:1px dashed var(--muted)"></span>Next window</span><span class="row" style="gap:4px"><span style="width:10px;height:10px;border-radius:2px;background:var(--danger)"></span>At limit</span></span></div>
<div class="box"><div class="row xs muted" style="padding:8px 16px 0 140px;justify-content:space-between"><span>Now</span><span>16:00</span><span>20:00</span><span>00:00</span><span>04:00</span><span>08:00</span><span>12:00</span></div>
{tl_row("work","a-orange",[(26,21,"rgba(116,173,232,.55)"),(47,21,"rgba(116,173,232,.15)")])}
{tl_row("personal","a-blue",[(30,21,"rgba(222,193,132,.6)"),(51,21,"rgba(116,173,232,.15)")])}
{tl_row("client","a-green",[(25,10,"rgba(116,173,232,.55)"),(35,13,"var(--danger)"),(48,21,"rgba(116,173,232,.15)")])}
{tl_row("codex","a-violet",[(28,21,"rgba(116,173,232,.55)")])}</div>
<div class="row" style="gap:16px;align-items:flex-start"><div style="flex:1"><div class="section-h">{i("warn")}<h2>Watch list</h2></div>{rows_box([
 f'<div class="row" style="padding:10px 16px"><span class="sm grow">personal · weekly</span><span class="xs muted">Running low until Sat 20:00</span></div>'])}</div>
<div style="flex:1"><div class="section-h">{i("clock")}<h2>Recent resets</h2><span class="xs muted">Last 7 days</span></div>{rows_box([
 f'<div class="row" style="padding:10px 16px"><span class="sm grow">client · 5-hour</span><span class="xs muted">Hit the limit · reset 09:36</span></div>',
 f'<div class="row" style="padding:10px 16px"><span class="sm grow">work · weekly</span><span class="xs muted">Peaked at 81% · reset Mon 09:00</span></div>'])}</div></div>'''
SCREENS["c1-timeline-2"] = c1_page(timeline, "Timeline")

def heat():
    import random
    random.seed(4)
    cells = []
    for d, day in enumerate(["Mon","Tue","Wed","Thu","Fri","Sat","Sun"]):
        row = [f'<span class="xs muted" style="width:32px">{day}</span>']
        for h in range(24):
            v = max(0, (1 - abs(h - 14) / 7) * (0.4 if d > 4 else 1) + random.uniform(-.15, .15))
            row.append(f'<span style="width:18px;height:14px;border-radius:2px;background:rgba(116,173,232,{0.06 + v*0.8:.2f})"></span>')
        cells.append(f'<div class="row" style="gap:2px">{"".join(row)}</div>')
    return "".join(cells)
analytics = f'''<div class="row"><span class="seg"><span>30 days</span><span class="on">90 days</span></span><button class="btn xs">{i("users")}All accounts{i("chev")}</button><div class="grow"></div></div>
<div class="row" style="gap:12px">{tile("Forecast · personal", "Runs out Fri 18:00", "1.6× an even pace, 2 days before its weekly reset")}{tile("Busiest hour", "Tue 14:00", "Weekday afternoons hold 61% of use")}{tile("Hit the limit", "7 times", "client 5, personal 2, in 90 days")}</div>
<div><div class="section-h">{i("flame")}<h2>When work happens</h2><span class="xs muted">5-hour use by weekday and hour, local time</span></div><div class="box" style="padding:16px">{heat()}</div></div>
<div class="row" style="gap:16px;align-items:flex-start"><div style="flex:1"><div class="section-h">{i("gauge")}<h2>Plan fit</h2></div><div class="box"><table><tr><th>Account</th><th class="r">Avg weekly peak</th><th class="r">Hits</th><th>Verdict</th><th class="r">Unused / month</th></tr>
<tr><td>work</td><td class="r">58%</td><td class="r">0</td><td>Fits</td><td class="r">$84</td></tr><tr><td>personal</td><td class="r">92%</td><td class="r">2</td><td>Often at its limit</td><td class="r">$2</td></tr><tr><td>codex</td><td class="r">21%</td><td class="r">0</td><td>Could downsize</td><td class="r">$16</td></tr></table></div></div>
<div style="width:340px;flex:none"><div class="section-h">{i("trend")}<h2>Move work</h2></div>{rows_box([f'<div class="row" style="padding:10px 16px;align-items:flex-start">{i("compare","i","margin-top:3px;color:var(--muted)")}<div class="col grow"><span class="sm">personal → work</span><span class="xs muted">personal runs out of its weekly limit before the reset; work has 59% left</span></div></div>'])}</div></div>'''
SCREENS["c1-analytics-2"] = c1_page(analytics, "Analytics")

wallet_rows = [
  ("work", "Max 20x", "$200 / month", "Renews Nov 3"), ("personal", "Pro", "$20 / month", "Renews Nov 12"),
  ("client", "Max 5x", "Included with harbor-labs", "—"), ("codex", "Plus", "Price not set", "")]
wallet = f'''<div class="row" style="gap:12px">{tile("Per month", "$220", "$2,640 a year · 3 of 4 priced")}{tile("Renewing in 30 days", "$220", "work on Nov 3, personal on Nov 12")}{tile("Top-ups this month", "$15", "1 expires in 3 days")}</div>
<div><div class="section-h">{i("wallet")}<h2>Subscriptions</h2><div class="grow"></div><span class="xs muted">Totals in USD · EUR at 1.08</span></div>{rows_box([
 f'<div class="row" style="padding:10px 16px;gap:12px"><span class="sm semi" style="width:120px">{n}</span><span class="xs muted" style="width:90px">{p}</span><span class="sm grow{" muted" if "not set" in c else ""}">{c}</span><span class="xs muted">{r}</span><button class="btn xs ghost">{i("dots")}</button></div>' for n,p,c,r in wallet_rows])}</div>
<div class="row" style="gap:16px;align-items:flex-start"><div style="flex:1"><div class="section-h">{i("plus")}<h2>Top-ups</h2><div class="grow"></div><button class="btn xs">{i("plus")}Add Top-Up</button></div>{rows_box([
 f'<div class="row" style="padding:10px 16px"><span class="sm grow">codex · $10 · 1,000 credits</span><span class="pill warn">{i("clock")}Expires in 3 days</span></div>',
 f'<div class="row" style="padding:10px 16px"><span class="sm grow">codex · $5 · Oct 2</span><span class="xs muted">Expires Jan 2</span></div>'])}</div>
<div style="width:360px;flex:none"><div class="section-h">{i("chart")}<h2>Paid top-ups per month</h2></div><div class="box" style="padding:16px;display:flex;align-items:flex-end;gap:12px;height:140px">{"".join(f'<div class="col" style="flex:1;align-items:center;gap:4px"><div style="width:100%;height:{h}px;border-radius:3px;background:var(--primary);opacity:{0.35 if h<4 else 1}"></div><span class="xs muted">{m}</span></div>' for m,h in [("Jun",2),("Jul",30),("Aug",0),("Sep",45),("Oct",22),("Nov",68)])}</div></div></div>'''
SCREENS["c1-wallet-2"] = c1_page(wallet, "Wallet")

if __name__ == "__main__":
    only = sys.argv[1:]
    for name in [n for n in SCREENS if n.startswith("c1-")]:
        if not only or name in only:
            write(name, SCREENS[name])
