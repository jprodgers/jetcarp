# Decisions

What the people working on JETCARP have decided, and why. JETCARP's code is written by an AI coding
assistant, so this page records what the commit history cannot show: which choices were a person's,
what else was considered, and where the assistant proposed something that a person then accepted.
Today "the people" is one person, Jimmie Rodgers. The page is kept so that it still works when
there are more.

## How this page is kept

- One entry per decision. Entries are numbered and never renumbered.
- A decision that changes gets a new entry, which names the one it replaces. The old entry stays,
  marked as replaced.
- Each entry says who decided and when. An entry written down after the fact says so.
- **Claude's part** says where an idea, or the options, came from the assistant. Where an entry has
  no such line, there was none.
- Words in quotation marks are the decider's own.
- Decisions about how releases are tested and what the test records publish are in the test
  records' own page: [jetcarp-testing/DECISIONS.md](https://github.com/jprodgers/jetcarp-testing/blob/main/DECISIONS.md).

**How the first entries were written.** On 18 November 2026, during the independent audit of 0.4.2,
the auditor (Claude, in Claude Code) interviewed Jimmie in writing and drafted D-01 to D-36 from
his answers and from the public record. Jimmie corrected and approved them. Quotes with no source
named are from that interview. Decisions made before that day are dated as closely as the record
allows.

## Why JETCARP exists

### D-01. Build a new program

- **Decided:** Jimmie, early November 2026, when the prototype worked (D-08). Written down
  18 November 2026.
- **Why:** "I've been playing around with CAs since the early 2000s, when they were introduced to
  me in college. I've been writing software and playing with software for CAs ever since, but none
  of my software ever ran as efficiently as I had hoped, as I'm calculating tons of extra
  information on top of standard CAs to allow for the coloring and layering of them." By layering
  he means "layering multiple rulesets on top of one another, running one for X generations per Y
  generations of another" (D-13). He also wanted to make art with it, "which the other projects
  don't directly support." Of the tools he tried: "I wasn't able to change color palettes to my
  liking in LifeViewer, and I was limited to only certain types of CAs." Then the prototype "ran
  buttery smooth, and did what I wanted right out of the gate. It was then that I decided to move
  forward with making the prototype a real project and release it properly. It was more accident
  than intentional those first few days, but a happy one that I stand by."
- **Considered:** extending Golly or Ready. Ruled out by their licence (D-02).
- **How far this goes:** what is new is claimed "to my knowledge". Jimmie has not used Golly's
  layers and cannot say how they compare with JETCARP's several automata at once. The book lists
  what is not new: Ready runs arbitrary rules on meshes, and LifeViewer coloured cells by age first
  ([What is new here](https://jprodgers.github.io/jetcarp/automata/generalising.html#what-is-new-here),
  [What we do not claim](https://jprodgers.github.io/jetcarp/research/contributions.html#what-we-do-not-claim)).
- **Stands.**

### D-02. No copyleft code; other programs may be run and compared with

- **Decided:** Jimmie. His practice for years, applied to JETCARP from the start. Written down
  18 November 2026.
- **The decision:** JETCARP contains no code, rule files, patterns or text from a work under a
  copyleft licence, Golly and Ready among them, and no such code is read as a model for
  JETCARP's. Running those programs, comparing results with them, and reading or writing their
  documented file formats is fine.
- **Why:** "Philosophically I wanted to release under the Unlicense … I feel that science tools and
  literature should be public domain. I didn't touch Golly's code for precisely this reason, and I
  won't touch any GPL project for this reason." And: "I'm fine running GPL software, and anything I
  can do to externally validate JETCARP is good. I don't mind comparing, I just don't want to get
  caught up in a licensing dispute."
- **In the record:** the provenance audit before release found two pieces that followed other
  people's code too closely, and both were rewritten
  ([How JETCARP was made](https://jprodgers.github.io/jetcarp/about/how-made.html#the-provenance-audit));
  [Contributing](https://jprodgers.github.io/jetcarp/about/contributing.html) says what can be
  included.
- **Stands.**

### D-03. Public domain: the Unlicense, with the MIT licence as a fallback

- **Decided:** Jimmie, before the first release (13 November 2026).
- **Why:** "I basically either keep it private, or go full public domain, I don't generally mess
  around in the middle. So I've been about this life for a long time." He names the design of a
  tattoo, released in 2013, as an early example
  ([Wired, January 2013](https://www.wired.com/2013/01/resistor-code-arm-tattoo/)). And of this
  program: "I also don't wish to take credit or hold a license for software I didn't write, and
  since Claude can't hold that license, public domain it is." The MIT licence "is in there for
  jurisdictions that don't recognize" a public-domain dedication.
- **Also:** data from JETCARP's own searches is released under CC0.
- **In the record:** [Licence](https://jprodgers.github.io/jetcarp/about/licence.html).
- **Stands.**

### D-04. Rust, as a desktop program

- **Decided:** Jimmie, early November 2026 (the prototype was already in Rust).
- **Why:** "For speed, efficiency, and large/complex scene support. Most of the CAs I like to see
  run take actual computing power. I have quite a bit of compute at my disposal, so I wanted a
  desktop application that could fully take advantage of it. I've become a huge fan of Rust over
  the past year."
- **Considered:** his own earlier programs were in JavaScript and Python. A Python interface is on
  the roadmap ([#164](https://github.com/jprodgers/jetcarp/issues/164)). "Eventually we'll get
  JETCARP running in the browser, but that isn't a priority right now."
- **Stands.**

### D-05. Claude writes the code; Jimmie directs and tests

- **Decided:** Jimmie, before the prototype. Written down 18 November 2026.
- **Why:** "I'm terrible at coding for the most part. I'm also quite slow, and only just started
  learning Rust a year ago. After two years of building prototypes with agentic coding, I've come
  to find that I'm best directing and testing." "I do read Rust, I just don't code well in it. Same
  with most languages for me, input is easy, output is hard." "I chose to have Claude write it for
  speed and efficiency, and to test it to hell and back."
- **What follows from it:** "I don't take the code on trust; that's why I test so heavily, and make
  the claims I make." The testing is D-27 and D-28; the claims are D-15.
- **In the record:** [How JETCARP was made](https://jprodgers.github.io/jetcarp/about/how-made.html).
- **Stands.**

### D-06. What Jimmie reads and checks himself

- **Decided:** Jimmie; his practice throughout. Written down 18 November 2026.
- **The chat and the changes:** "I read everything that Claude posts in chat (on all three systems,
  every message), and the diffs as they roll by. I keep up and comment as Claude goes, correcting
  things as I see the need."
- **The book:** "I've also read and approved of the full book, as well as made edits. I've read it
  back to front a few times while we have been working on it."
- **The code:** "The one thing I haven't spent much time reading is the code. I don't know Rust
  well enough to say if it's good or not, so I test to make up for that failing on my part."
  "I'll eventually read the whole code base, it's just very slow, and I've mostly been testing."
- **His own edits so far** were typed into the files while Claude worked, and went in with
  Claude's commits: "They are small, but numerous. I don't have good documentation on those small
  changes." So the history up to 0.4.2 does not show them: 610 of its 611 commits name Claude as
  co-author.
- **From 18 November 2026:** Jimmie aims to make his own edits as commits under his own name
  alone, so that the history shows them.
- **Stands.** See also D-33.

### D-07. Claude is credited, not an author

- **Decided:** Jimmie, before the first release.
- **The decision:** "Jimmie does not take credit for the code: the ideas and the direction are
  Jimmie's, the execution was done by agentic coding." Claude is acknowledged in the credits and is
  not listed as an author (the book,
  [How JETCARP was made](https://jprodgers.github.io/jetcarp/about/how-made.html)).
- **Stands.**

### D-08. A fresh start from the prototype, CART

- **Decided:** Jimmie, 10 November 2026, the day of JETCARP's first commit.
- **What CART was:** the prototype. "It was built in a single day by Claude as a proof of concept,
  found what was in the findings, and that's when I got more serious." It was then tested and added
  to for about a week. JACA first ran in it on 8 November 2026. Most of the finds in the book were
  made with it, and each was run again on JETCARP 0.4.0 before being reported.
- **Why a new repository, a new name and a new history:** "CART only included some of the features
  of JETCARP, also the name was well established, so I needed to change it." "CART lacked
  discipline and rigor, it was a simple prototype."
- **CART stays private and unchanged,** "which I'll share for auditors that really care."
- **Version numbers:** "I think of CART as v0.3.0, as I had a handful of other programs before it,
  and also it's partway towards my ideal goal, so I thought I would indicate that with the version
  number." So the first public release was 0.4.0.
- **Stands.**

### D-09. Made for art as much as for research, with games to come

- **Decided:** Jimmie, from the start. Written down 18 November 2026.
- **Who it is for:** "Me, quite frankly. I made it for me, and the art I want to make, and see in
  the world. No other program offers such an array of tools for CA based art, and I'm only getting
  started." The desktop background and the screen saver come from prototypes he had been running
  on his own computers. Beyond that: "I hope that anyone and everyone that has interest in either
  CAs or tilings will be drawn in."
- **Why puzzles are on the roadmap** ([#166](https://github.com/jprodgers/jetcarp/issues/166) to
  [#168](https://github.com/jprodgers/jetcarp/issues/168)): "The game was inspired heavily by the
  Natural Number Game from Lean. I used to work for the Museum of Science in Boston as an exhibit
  designer, so science education is near and dear to me. I think that a puzzle and logic game is
  the perfect way to learn about cellular automata and tilings, and a fantastic way to learn the
  program."
- **Stands.**

### D-10. The plane only

- **Decided:** Jimmie. Written down 18 November 2026.
- **The decision:** JETCARP runs on tilings of the Euclidean plane. Hyperbolic tilings and three
  dimensions are left out.
- **Why:** "It's mostly because the others are hard to visualize and navigate. I have a 3D CA
  prototype, and it works too, but it's not nearly as interesting, as the internal volume is
  difficult to see compared to having everything visible in a single grid. I may release the 3D
  tool at some point."
- **Stands.**

## How it is built

Jimmie on when these were settled: "It's a mixture as to when those questions were answered, some
were in CART, others weren't answered until JETCARP." All are from early November 2026, and none
is dated more closely than that.

### D-11. One neighbour graph for every tiling

- **Decided:** Jimmie set the requirement and adopted the model. Written down 18 November 2026.
- **How it came about:** "I asked if something like graph theory could apply to help generalize
  CAs, and then Claude basically wrote an artifact that showed it working without me asking for
  code. Once I saw that, I had Claude expand upon it, and a bit of back and forth built the model.
  It was my requirement to support as many CAs on as many tilings as possible. It was Claude that
  made it work from a single question during a conversation."
- **Claude's part:** the model itself, from Jimmie's question.
- **Stands.**

### D-12. Both kinds of neighbour, chosen freely

- **Decided:** Jimmie. Written down 18 November 2026.
- **The decision:** every rule can count either the cells that share an edge with a cell, or the
  cells that share an edge or a corner. The book calls these *edge* and *touch*.
- **Why:** "I phrased it in terms of expanded von Neumann and Moore neighborhoods, as different CAs
  are defined by that type of counting. I didn't use edge and vertex neighbors as the terminology,
  but it was very much my call. It was a hard requirement that both are supported, and that you
  can freely switch between them."
- **Stands.**

### D-13. Several automata on one tiling, each at its own pace

- **Decided:** Jimmie. It is what his earlier programs were for (D-01). Written down
  18 November 2026.
- **The decision:** a world can hold any number of automata, on shared or separate cells, each with
  its own rule, colouring and number of steps per generation
  ([Several automata at once](https://jprodgers.github.io/jetcarp/guides/several-automata.html)).
- **Stands.** The name clash between this "layering" and the *layered rules* is open (O-1).

### D-14. The same result on every computer, bit for bit

- **Decided:** Jimmie. Written down 18 November 2026.
- **Why:** "That was very much my idea. I said that it has to be reproducible on all systems. I was
  asked specifically if 'approximate' was ok for the CAs that use floating point, and I turned that
  down, opting for floating point that is accurate to the same degree on all systems. Even a tiny
  change can radically alter results, so I was fairly adamant about this one."
- **Considered:** approximate agreement for the rules that use real numbers. Turned down.
- **Claude's part:** asked the question, and built the arithmetic that meets the answer.
- **In the record:** [Reproducibility](https://jprodgers.github.io/jetcarp/research/reproducibility.html);
  the parity sets in the test records.
- **Stands.**

### D-15. Nothing is catalogued unverified, and nothing is claimed that we have not checked

- **Decided:** Jimmie. Written down 18 November 2026.
- **Why:** "I want results to be solid. If they can't hold up under our own scrutiny, it has no
  hope in the wild."
- **In the record:** every catalogue entry is run again and compared cell by cell before it goes
  in. A find is claimed only after a logged search for earlier work, in the words "to our
  knowledge"; a doubtful one is flagged, not claimed. Jimmie decides what is claimed
  ([How JETCARP was made](https://jprodgers.github.io/jetcarp/about/how-made.html#how-the-claims-were-checked)).
- **Stands.**

### D-16. An app, a command line, scripting and an MCP server, over one shared layer

- **Decided:** Jimmie. Written down 18 November 2026.
- **How it came about:** "The request from me was to have all of those things, and options were
  presented with pros and cons. I chose the shared layer option."
- **Claude's part:** set out the options.
- **Stands.**

### D-17. Searches survive being stopped

- **Decided:** Jimmie approved it. Written down 18 November 2026.
- **How it came about:** "That was Claude's suggestion after a search got canceled and couldn't be
  resumed during testing, and I approved the feature and it got added pretty early in the
  process."
- **Claude's part:** the suggestion.
- **Stands.**

### D-18. Which tilings: every tiling by regular polygons, in the end

- **Decided:** Jimmie. Written down 18 November 2026.
- **The goal:** every tiling of the plane by regular polygons, "or at least a program that can
  display any and all." Any tiling at all "would be much more difficult."
- **Where it stops today:** "I supported what was available to me, with more planned in the
  future." The k-uniform tilings stop at k = 6 because the catalogue used stops there.
- **Stands.**

### D-19. JACA

- **Decided:** Jimmie; it is his idea. First run in CART on 8 November 2026.
- **Where it came from:** "JACA was an idea I had based off of a balanced ternary CA I had built in
  a 3D visualizer. It worked ok, but once I put it on a plane and got rules that worked well
  together, I became very pleased and surprised it worked. I have a minor obsession with ternary."
  The years before were "spent playing around with whatever I could get my hands on", and "JACA in
  its full form didn't come until CART."
- **How far this goes:** "a CA I believe is my own idea, I haven't found prior art for it." The
  book credits each ingredient that has a precedent
  ([JACA](https://jprodgers.github.io/jetcarp/automata/jaca.html#where-it-came-from)).
- **Stands.**

### D-20. An AI client can drive JETCARP, within limits

- **Decided:** Jimmie. Written down 18 November 2026.
- **Why:** "I do lots of my research with AI" (another open project of his is
  [CITAR](https://github.com/jprodgers/CITAR)). "I find explaining in natural language what I want
  a script to do is faster and more accurate in the long term. I also wanted to make this more
  accessible, and I thought AI assisted research would help with that."
- **The limits, from the record:** a client's tools read and write files only in JETCARP's output
  folder, the library's folder and folders the user names; the app accepts commands only while AI
  control is switched on; a client cannot make the pack-signing key
  ([SECURITY.md](https://github.com/jprodgers/jetcarp/blob/main/SECURITY.md)).
- **Stands.**

## Safety, privacy and openness

### D-21. A photosensitive seizure warning before anything moves, and a guard to follow

- **Decided:** Jimmie, 15 or 16 November 2026, during the testing of 0.4.2's third candidate
  ([#96](https://github.com/jprodgers/jetcarp/issues/96)).
- **The decision:** the warning must be answered before anything animates on screen. A strobe guard
  that measures flashing and softens, slows or pauses a scene is to follow, switched on by default
  ([#97](https://github.com/jprodgers/jetcarp/issues/97)).
- **Why:** "I've personally experienced a seizure, though I'm not sensitive to strobing. It put me
  in the hospital, and I very nearly died. I had to go through physical therapy to walk and stand
  again. The thought of causing that for another person is horrifying for me."
- **How it came about:** "I was watching Hodgepodge on my large monitor, mesmerized, and thought to
  myself 'good thing I don't have a photosensitive seizure disorder', and then thought immediately
  afterwards that I needed to do something about it. So the very next prompt to Claude was about
  the seizure warning, with a goal of a detector built in. This happened during testing, hence it
  not being in v0.4.0."
- **Stands.**

### D-22. Nothing is collected about users, ever

- **Decided:** Jimmie. Written down in these words 18 November 2026.
- **The decision:** "I will never collect any stats on any users, how they use it, where they use
  it, or anything about them beyond the data point of a download has occurred. I won't collect
  emails, or even the countries that they are coming to me from."
- **In the record:** JETCARP makes no network connections, checks for no updates and reports
  nothing ([SECURITY.md](https://github.com/jprodgers/jetcarp/blob/main/SECURITY.md)).
- **Stands,** whatever is decided about networking (D-23).

### D-23. Networking and a built-in BitTorrent client: proposed, not decided

- **Proposed by:** Jimmie. Tracked in [#169](https://github.com/jprodgers/jetcarp/issues/169) and
  [#170](https://github.com/jprodgers/jetcarp/issues/170).
- **Where it stands:** "I'm not 100% on the built in client quite yet, but I wanted it in the
  roadmap for discussion, so it's going to come down to the community." It was first planned for
  0.5.0 and moved to 0.8.0, "as I want to give time to talk about it. Also it's going to open a
  whole can of security worms that I'm not keen to open quite yet."
- **Why it appeals:** "I like distributed systems like BT for large files and the like, so the idea
  of being able to do the research, sign and release a pack, and have that distributed in the same
  program is very appealing to me."
- **Already promised:** any networking "will happen only when you ask for it"
  ([SECURITY.md](https://github.com/jprodgers/jetcarp/blob/main/SECURITY.md)), and D-22 holds.
- **Open** (O-2).

### D-24. Work in the open, and say plainly what was and was not done

- **Decided:** Jimmie, before the first release. Written down 18 November 2026.
- **Why:** "I want to be very clear about what I have and haven't done, what I have and haven't
  found, and what I have and haven't read or tested. I'm developing in the open for everyone to
  see, audit, and verify. I don't wish to mis-label or mislead."
- **What is public:** the code and its history, the book with its page of mistakes, every problem
  as an issue, the test records, and the audits.
- **What is kept private, and why:** the chats with Claude ("I talk to Claude about personal issues
  mixed in with the chats", and they hold "the actual file-paths and such on my computers");
  personal information; CART (D-08); and the details of a security problem until it is fixed
  (D-25).
- **Stands.**

### D-25. A security problem is handled privately until a release fixes it

- **Decided:** Jimmie, before the first release.
- **The decision:** security problems are reported and fixed privately, and described in public,
  with an advisory, once a release carries the fix. Every other problem gets a public issue.
- **In the record:** [SECURITY.md](https://github.com/jprodgers/jetcarp/blob/main/SECURITY.md);
  [How JETCARP is tested](https://jprodgers.github.io/jetcarp/about/testing.html).
- **Stands.**

### D-37. The dependencies are checked, and networking comes only on purpose

- **Decided:** Jimmie, 19 November 2026, on the pull request that added the check
  ([#187](https://github.com/jprodgers/jetcarp/pull/187)).
- **The decision:** CI checks the dependency tree with cargo-deny on every push, every pull
  request and once a week: known vulnerabilities, network client and server crates, and crates
  from anywhere but crates.io. Networking is not ruled out: a change that adds it allows its crate
  in `deny.toml`, for the one JETCARP crate that uses it, and updates SECURITY.md.
- **Why:** "When I said a while back that JETCARP makes no network connection, I only meant that
  currently. … I don't mean that for all time, and never have." And: "If it doesn't prevent us from
  adding networking components, and there's a way to allow for known networking to occur in the
  ways we specify, then that's fine. Otherwise I don't want to force us into no networking long
  term, that's not my goal."
- **How it reports:** "Required, later": the check only reports for now; which checks must pass
  before `main` moves is decided for all of CI's jobs together. A notice that a crate is no longer
  maintained is a warning and "never fail"s the check ([#186](https://github.com/jprodgers/jetcarp/issues/186)
  lists them).
- **Claude's part:** the check, and the way to allow a network crate for one JETCARP crate only
  (cargo-deny's `wrappers`), came from the plan of the security work after the 0.4.2 audit; the
  two answers in quotation marks are the options Jimmie chose from the ones Claude offered.
- **Stands.** It works with D-23: whatever networking is decided, it comes in the crate meant
  for it.

### D-41. Uninstalling deletes only a data folder it can recognise as JETCARP's

- **Decided:** Jimmie, 19 November 2026 ([#195](https://github.com/jprodgers/jetcarp/issues/195)).
- **The decision:** "Refuse unless it's JETCARP's": `jetcarp uninstall --data` deletes the data
  folder only when it can recognise it as JETCARP's (its library, its own sub-folders, or a
  marker); a folder of other files is refused, named, and left for the person to delete by hand.
- **Why:** testing found that a data folder set to a folder of someone's documents was deleted
  whole, as told. The plan named the folder first, but the deletion cannot be undone.
- **Claude's part:** the test, and the three options (refuse; warn louder; leave it); Jimmie
  chose the first.
- **Stands.** The fix is due in 0.4.3.

## Releases

### D-26. What does not work as intended does not ship

- **Decided:** Jimmie. Applied on 16 and 17 November 2026; written down 18 November 2026.
- **The rule:** "Anything that doesn't work as intended doesn't ship."
- **Applied:** "Fit to this computer" was taken out of 0.4.2 on Jimmie's rule: "A fit should be 10
  seconds, maybe 30 seconds. If we can't hit that target, I want the feature removed"
  ([#112](https://github.com/jprodgers/jetcarp/issues/112); the fast fit is
  [#113](https://github.com/jprodgers/jetcarp/issues/113)). "Claude couldn't get the auto fit to
  work, so we pushed it back as well. That was found in testing, and we had an extra release
  candidate and an extra day of testing because of it too." A window that froze after being
  minimised on macOS ([#131](https://github.com/jprodgers/jetcarp/issues/131)) was the reason for
  a fifth candidate.
- **What may wait:** on 15 November 2026 Jimmie set how fixes are divided: "every fix that
  reasonably can go into rc.3 does; rc.4 catches stragglers; anything too big goes to 0.4.3." A
  problem that waits keeps its open issue.
- **Stands.**

### D-27. A release candidate is tagged only on Jimmie's go

- **Decided:** Jimmie, for the 0.4.2 cycle (November 2026).
- **The decision:** a candidate is tagged only when the automated tests pass on Linux, Windows and
  macOS, and only when Jimmie says so
  ([How JETCARP is tested](https://jprodgers.github.io/jetcarp/about/testing.html)).
- **Stands.** See D-35 for who may say so in future.

### D-28. Every release is tested on real machines before it ships

- **Decided:** Jimmie.
- **The decision and its reasons** are in the test records' page,
  [jetcarp-testing/DECISIONS.md](https://github.com/jprodgers/jetcarp-testing/blob/main/DECISIONS.md).
  In short: every operating system and every GPU vendor he can reach, the upgrade path, a
  newcomer's first hour, and an issue for every problem, because he does not take the code on
  trust (D-05).
- **Stands.**

### D-29. What 1.0 will mean

- **Decided so far:** 1.0 is the release that declares the scene format, the command line, the
  scripting API and the MCP tools stable
  ([Changelog](https://jprodgers.github.io/jetcarp/about/changelog.html)).
- **Not decided:** what else it must have. "I'm honestly not sure what all needs to be in a 1.0,
  I'm still defining some of that."
- **Open** (O-3).

### D-30. One large feature per release

- **Decided:** Jimmie, November 2026. Written down 18 November 2026.
- **The decision:** "I tried to keep it to only one large feature per bump, and any supporting
  features going in where appropriate. Bugfixes happen as soon as I can get them in."
- **How the order was set:** "That order came from what Claude said it could build, and how heavy
  of a task each one was."
- **Claude's part:** the estimates of how large each task is.
- **Stands.** The roadmap is the tracking issues; a page for it in the book is
  [#151](https://github.com/jprodgers/jetcarp/issues/151).

### D-31. Publish the work, and submit it to JOSS

- **Decided:** Jimmie, November 2026.
- **Why:** "I hope this is able to move the field of research forward, and if not, at least it will
  make some cool art along the way." "I'm doing research, I might as well share it. If JOSS
  refuses me, I'm still going to move forward."
- **Who uses it so far:** "friends, family, and some users from the ConwayLife forums, as I've been
  active there since releasing."
- **Stands.** What stands between JETCARP and a submission is tracked in
  [#184](https://github.com/jprodgers/jetcarp/issues/184).

### D-32. An independent audit every three to six months, published in full

- **Decided:** Jimmie, 18 November 2026.
- **The decision:** a fresh session with no part in writing the program audits it every three to
  six months, and the report is published whole: "the good, the bad, the ugly." Bugs and
  documentation errors become issues. Privacy and security findings go to the maintainer
  privately; the public report says that there were some and what was done, not what they were.
- **In the record:** [jetcarp-testing/audits](https://github.com/jprodgers/jetcarp-testing/tree/main/audits).
- **Stands.**

### D-33. People who know the field should check it too

- **Intended by:** Jimmie. Written down 18 November 2026.
- **The intention:** someone who knows Rust well reads the code with Jimmie ("I hope to find
  someone that does know Rust to go over it with me at some point"), and a person reviews its
  security. "Right now just hopes, but before JOSS at a minimum."
- **Open** (O-5): nobody has been found yet.

### D-38. The workflows' actions are pinned, and updated each release cycle

- **Decided:** Jimmie, 19 November 2026 ([#192](https://github.com/jprodgers/jetcarp/pull/192)).
- **The decision:** every action the workflows use is named by its commit, with its version
  beside it, so that a tag moved by an action's owner cannot change what runs in the release job.
  "Update the hashes each release cycle": one pull request at the start of each cycle, reviewed by
  Jimmie.
- **Claude's part:** the pinning came from the audit's issue
  [#183](https://github.com/jprodgers/jetcarp/issues/183); Claude offered updating by hand each
  cycle or by Dependabot, and Jimmie chose by hand.
- **Stands.**

### D-39. Release files: checksums by the installers, attestations by hand, Windows not yet signed

- **Decided:** Jimmie, 19 November 2026 ([#191](https://github.com/jprodgers/jetcarp/pull/191)).
- **The decision:** the installers check a download against `SHA256SUMS`. Every release file
  also has a provenance attestation, which a person checks with `gh attestation verify`; the
  installers do not check it themselves ("Not now; say how"), to be looked at again with the
  networking design. Windows signing is "Not live yet": the release workflow's SignPath step is
  ready and switched off, and SECURITY.md says so.
- **Claude's part:** the options; Jimmie chose.
- **Stands.**

### D-40. An outside score and code scanning: later, together

- **Decided:** Jimmie, 19 November 2026.
- **The decision:** "Later, both": the OpenSSF Scorecard (a public score) and GitHub's code
  scanning are added together once the security work's pull requests are merged and the checks
  required before `main` moves are decided, so that the first public score reflects them. Both
  were tried privately first.
- **Claude's part:** the trial and the options.
- **Open** until then.

### D-42. 0.4.3: the audit's security problems first, and every bug fix that can go in

- **Decided:** Jimmie, 19 November 2026.
- **The decision:** "I want to make sure all the security issues brought up by the audit are
  addressed before starting 0.4.3. I also want to make sure that any and all bug fixes that can go
  in, do go in. If that makes this a big release, then so be it."
- **Stands.** It applies D-26's rule ("Anything that doesn't work as intended doesn't ship") to
  the next release as a whole.

## People

### D-34. Contributions, AI-written ones included, are welcome and are verified

- **Decided:** Jimmie, before the first release.
- **The decision:** "I welcome AI written code that follows our guidelines and does what it says on
  the tin." A contributor says in the pull request that AI was used, and must understand, have run
  and be able to explain every change. Every contribution is reviewed with AI tools and then
  tested by Jimmie: "I will be using AI tools to evaluate, and then I will test personally."
- **In the record:** [Contributing](https://jprodgers.github.io/jetcarp/about/contributing.html).
- **Stands.**

### D-35. Who decides

- **Decided:** Jimmie, 18 November 2026.
- **The decision:** for now Jimmie decides, and says "go" on each release. "We'll see how involved
  others get. I don't need to maintain absolute control, I wouldn't have public domain-ed it if I
  did, so I'm open to others having that power as well."
- **Open** in part (O-4): how someone else comes to share that power is not yet written.

### D-36. Where to ask for help

- **Decided:** Jimmie.
- **The decision:** questions go to
  [Discussions](https://github.com/jprodgers/jetcarp/discussions); problems go to
  [issues](https://github.com/jprodgers/jetcarp/issues). Both are open to anyone.
- **To do:** say so in the README and in Contributing
  ([#184](https://github.com/jprodgers/jetcarp/issues/184)).
- **Stands.**

## Open

Decisions not yet made. Each moves up into the list above, with a number, when it is.

- **O-1. A new name for the layered rules.** Jimmie uses "layering" for several automata running
  together (D-13), and "Layered Cellular Automata" is already the name of a different idea in the
  literature, as the rules' own page notes. A new name needs the old notation (`Layer 2/01`) kept
  as an alias, so that saved scenes still load.
- **O-2. Networking and a built-in BitTorrent client** (D-23).
- **O-3. What 1.0 must contain** (D-29).
- **O-4. How others come to share decisions** (D-35).
- **O-5. A Rust reviewer and a security reviewer** (D-33).
