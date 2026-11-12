# Security

## Reporting a problem

If you find a security problem in JETCARP, please email Jimmie Rodgers at
<jimmie@jimmieprodgers.com> rather than opening a public issue, and give Jimmie a chance to fix it
before you make it public. Say which version you use (`jetcarp --version`), on which system, and
how to reproduce the problem. Fixes go into the next release, with credit to you if you would like
it. JETCARP is research software maintained by one person: there is no bug bounty, and only the
newest release gets fixes.

## What JETCARP does, and does not do

**It makes no network connections.** JETCARP 0.4 contains no HTTP client, no DNS lookups and no
TLS: it never connects to the internet, checks for updates, or sends anything anywhere. There is
no telemetry, no analytics and no crash reporting. The only sockets it opens are local ones, on
`127.0.0.1`, described below. Packs and other files reach it only when you give them to it.
Networking (downloading packs, for one) is planned for a later version, and will happen only when
you ask for it.

**Scripts in packs never run by themselves.** A pack is a folder of data: Parquet tables, JSON
documents and pictures, each stored under its SHA-256. Nothing in a pack is executable, nothing is
run from one, and `jetcarp pack check` and `jetcarp pack import` check every file's hash and every
signature before anything is stored. In this version a scene holds no code at all: its `script`
is a list of steps and the conditions that move between them, which is data. The pack format
requires that if a later version lets scenes carry Rhai scripts, a script from a pack does not run
unless you switch scripts on for that pack.

**Rhai scripts run only when you run them** (`jetcarp script FILE`, from the app, or through the
MCP server's `run_script` tool), and they write files only inside JETCARP's output folder or the
folder you give with `--out`.

**The local link and its token.** The running app (only while *AI control* is switched on) and the
background host accept commands over TCP on `127.0.0.1`, which other computers cannot reach. A
connection must first send a 128-bit token, which the app or host makes afresh each time it starts
listening, and is closed if the token is wrong (the comparison takes the same time whichever
character is wrong). The port and the token are kept in a file in JETCARP's runtime folder
(`$XDG_RUNTIME_DIR/jetcarp`, else `~/.cache/jetcarp`, on Linux and macOS; `%LOCALAPPDATA%\jetcarp`
on Windows), which only your user can read (on Windows, through the permissions of your own
profile folder). Anything that can read that file,
which means any program running as you, can control JETCARP; that is the boundary the token draws.
`jetcarp mcp`, the MCP server, reads that file to reach the app or host, and otherwise talks only
over its standard input and output to the AI client that started it.

**An AI client connected to JETCARP can do what its tools allow:** change and run worlds, run
searches, write pictures and run Rhai scripts, within the same folder limits as you. Connect only
clients you trust, and switch AI control off when you are not using it.

**Files it writes** are listed in [Folders and environment variables](docs/src/reference/folders.md)
and [Uninstalling](docs/src/guides/uninstalling.md): its settings, data and output folders, the
runtime folder above, and, when you ask for them, a login entry, a GNOME Shell extension, a screen
saver and a desktop picture.

## Release files

Every release lists the SHA-256 of each file in `SHA256SUMS`, and the installers check the archive
they download against it. How to check a download yourself is in
[Installing JETCARP](docs/src/getting-started/installing.md#checking-a-download). Releases for
macOS are not signed with an Apple Developer ID, and releases for Windows are not yet signed; the
same page says how to open them.
