# Security

## Reporting a problem

If you find a security problem in JETCARP, please report it privately rather than opening a public
issue: either with **Report a vulnerability** on the repository's
[Security tab](https://github.com/jprodgers/jetcarp/security/advisories/new), or by email to Jimmie
Rodgers at <jimmie@jimmieprodgers.com>. Give Jimmie a chance to fix it before you make it public. Say which version you use (`jetcarp --version`), on which system, and
how to reproduce the problem. Fixes go into the next release, with credit to you if you would like
it. JETCARP is research software maintained by one person: there is no bug bounty, and only the
newest release gets fixes.

## What JETCARP does, and does not do

**It makes no network connections.** JETCARP 0.4 contains no HTTP client, no DNS lookups and no
TLS: it never connects to the internet, checks for updates, or sends anything anywhere. There is
no telemetry, no analytics and no crash reporting. The only network sockets it opens are local
ones, on `127.0.0.1`, described below; its one HTTP server, the MCP server's endpoint, listens only
when you start it. (On Linux the windowing libraries also reach the display
server and the desktop's message bus, through local sockets that are not network sockets.) Packs and other files reach it only when you give them to it.
Networking (downloading packs, for one) is planned for a later version, and will happen only when
you ask for it.

**Scripts in packs never run by themselves.** A pack is a folder of data: Parquet tables, JSON
documents and pictures, each stored under its SHA-256. Nothing in a pack is executable, nothing is
run from one, and `jetcarp pack check` and `jetcarp pack import` check every file's hash and every
signature before anything is stored. In this version a scene holds no code at all: its `script`
is a list of steps and the conditions that move between them, which is data. The pack format
requires that if a later version lets scenes carry Rhai scripts, a script from a pack does not run
unless you switch scripts on for that pack.

**Rhai scripts run only when you run them** (`jetcarp script FILE`, from the app, through the
MCP server's `run_script` tool, or as a rotation's script while that rotation plays), and they
write files only inside JETCARP's output folder or the folder you give with `--out` (through the
MCP server, also the folders you allow with `--allow`).

**The local link and its token.** The running app (only while *AI control* is switched on) and the
background host accept commands over TCP on `127.0.0.1`, which other computers cannot reach. A
connection must first send a 128-bit token, which the app or host makes afresh, from the operating
system's random source, each time it starts listening; a connection with a wrong token is told so
and closed (the comparison takes the same time whichever character is wrong). The port and the
token are kept in a file in JETCARP's runtime folder (`$XDG_RUNTIME_DIR/jetcarp`, else
`~/.cache/jetcarp`, on Linux and macOS; `%LOCALAPPDATA%\jetcarp` on Windows; `JETCARP_RUNTIME_DIR`
when set), which only your user can read: on Linux and macOS the folder is made private (mode
0700) and the file too (0600), and both are checked, so the file is not left behind when they
cannot be; on Windows, through the permissions of your own profile folder. Anything that can read
that file, which means any program running as you, can control JETCARP; that is the boundary the
token draws. `jetcarp mcp`, the MCP server, reads that file to reach the app or host, and
otherwise talks only to its AI client: over its standard input and output, or, when you start it
with `--http`, over the HTTP endpoint below.

**The MCP server's HTTP endpoint.** `jetcarp mcp --http`, and the app while you tick *Also serve
MCP over HTTP* in its AI control window, serve the MCP server over HTTP, for AI clients that connect
to a URL. Nothing listens for HTTP unless you start it so: the app's endpoint closes when you untick
it or switch AI control off, and is off again at the next launch. It is bound to `127.0.0.1`, which
other computers cannot reach. Every request must carry a 128-bit token as `Authorization: Bearer`,
made afresh, from the operating system's random source, each time the endpoint starts (unless you
keep it, below), and recorded with its URL in a file in the runtime folder above that only your
user can read, as the link's token is; a request without it, or with another, is refused, and the
comparison takes the same time whichever character is wrong. So that no web page can reach it
through your browser, a request must be addressed to `127.0.0.1` or `localhost` on its port (its
`Host`), any `Origin` must be the endpoint's own or one you have allowed (below), bodies must be
JSON, and no answer lets any other origin read it: a page that has its own name resolve to
`127.0.0.1` (DNS rebinding) is refused. Requests are limited in size (heads of 16 KiB, bodies of
16 MiB) and time, and to 16 connections at once. A client over HTTP has the same tools, on the same
folders, as one over standard input and output.

Two choices loosen this, for convenience, and both are off unless you make them:

- **Keeping the token** (`jetcarp mcp --http --keep-token`, or *Keep this token for next time* in
  the app) means a client need not be given a new token each time the endpoint starts. The token is
  kept in a private file in JETCARP's settings folder (`http-tokens/`, a folder readable by you
  alone: on Linux and macOS mode 0700, the file 0600; on Windows through your profile folder's
  permissions), and is taken back only if the file is still readable by you alone and was written
  as kept. JETCARP says so each time it keeps one: on the command line in one line of its standard
  error, in the app under the box. What it gives up: a kept token survives restarts, logouts and
  reboots, and stays valid until you make a new one (`--new-token`, or *New token* in the app,
  which refuses the old one at once and replaces the file; turning keeping off deletes it). A token
  that never changes is worth more to anyone who sees it (a screen share, a copied client
  configuration, a program running as you that reads the file, a backup or copy of your settings
  folder; on Windows the settings folder is in your roaming profile, which some networks copy to
  other computers), and whoever has it keeps their access, whenever the endpoint runs, until then.
  `jetcarp uninstall --settings` deletes it with the rest of the settings folder.
- **Allowing an origin** (`--allow-origin URL`, or *Web pages that may connect* in the app) lets a
  browser-based client from that one origin connect, with the token, and answers its pages with
  the CORS headers browsers need. Only exact origins are accepted (no wildcards, no `null`), and
  every other origin is still refused. What it gives up: any page served from that origin, while
  it is open in your browser and has the token, can use the tools as you, so a site you allow that
  is compromised, or that serves other people's pages, is a way in. Allow only origins you
  control or trust, such as a client you run on this computer, and remove them when you are done.

**An AI client connected to JETCARP can do what its tools allow:** change and run worlds, run
searches, curate the library, write pictures and run Rhai scripts. Its tools read and write files
only in JETCARP's output folder, the library's folder and folders you name with
`jetcarp mcp --allow`; it cannot make your pack-signing key or record anyone's consent to publish
their ratings. Connect only clients you trust, and switch AI control and the HTTP endpoint off when
you are not using them.

**Files it writes** are listed in [Folders and environment variables](docs/src/reference/folders.md)
and [Uninstalling](docs/src/guides/uninstalling.md): its settings, data and output folders, the
runtime folder above, and, when you ask for them, a login entry, a GNOME Shell extension, a screen
saver and a desktop picture.

## Release files

Every release lists the SHA-256 of each file in `SHA256SUMS`, and the installers check the archive
they download against it, refusing one that does not match or is not listed. That shows a
download is complete and unchanged, but `SHA256SUMS` comes from the same place as the archive.
Where it came from is shown by a provenance attestation: the release workflow has GitHub make one
for every release file, a signed record of the repository, the workflow and the commit that built
it. The GitHub CLI checks it (`gh attestation verify FILE --repo jprodgers/jetcarp`); the
installers do not. How to check a download both ways is in
[Installing JETCARP](docs/src/getting-started/installing.md#checking-a-download).

Releases for macOS are not signed with an Apple Developer ID, and releases for Windows are not yet
signed: the release workflow has a signing step, through the SignPath Foundation, that is not
switched on yet. The same page says how to open them.
