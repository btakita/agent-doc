---
agent_doc_session: 76ecc7d3-a8f8-4db5-abfd-be1e69dcb082
tmux_session: '23'
agent_doc_mode: template
---

Please make a transcript of /home/brian/Videos/2026-03-12 00-43-08.mp4

This will be a YouTube video. Please title this accordingly.

Add a teaser about agent-doc and Proof that God Exists.


<!-- agent:exchange -->
## YouTube Video Details

**Title:** Dog-Fooding agent-doc: Live CRDT Debugging & A Proof That God Exists

**Description/Teaser:**

Watch a solo dev dog-food agent-doc — an AI-powered interactive document system — live, debugging CRDT merge issues, truncation detection, and reactive stream architecture with Claude as co-pilot. Along the way: a detour into Existence Lang, where God = Existence = the universal set... and yes, that means God's existence is provable by definition. Atheists welcome — your view exists too.

**What you'll see:**
- Real-time CRDT bug hunting across 10+ releases
- Template mode vs append mode vs stream mode architecture decisions
- Truncation detection heuristics for mid-sentence edits
- Reactive stream design (no debounce, CRDT handles concurrency)
- Existence Lang: a 14-term ontological kernel for modeling anything
- Why iterative development beats one-shotting
- Honest vibe-coding chaos — human-grade dog food

**Coming soon:** agent-doc + Existence Lang = executable ontology.

Like & subscribe. Fork the code. Or roast me in the comments — anything's better than being ignored.

**Tags:** agent-doc, Claude Code, CRDT, vibe coding, Rust, AI developer tools, Existence Lang, philosophy, ontology, dog-fooding, live coding, debugging

---

## Transcript

Hey guys, so yeah, I'm now debugging some interesting problem with the CRDT in Agent Doc. So basically, I'm kind of really in the thick of the debugging, so I figured, you know, I should've jumped in probably like a little while ago, but now's as good a time as any.

Basically, here's my Agent Doc — and it's my Agent Doc, debugging Agent Doc. So now I'm having an issue, a few issues. So one issue was I got a duplicate — I wrote a command and I got a duplicate response of the same command. So I ran Agent Doc compact over here, and it popped in twice. You don't see it now in this document because it actually ran the compact and that cleaned up everything, but I guess you're gonna have to trust me. I'm not gonna show you the git history, but it did duplicate it. So that's one of the issues we're kind of going through right now.

And as you can see, it's replacing — it kind of has a good analysis of what's going on. So yeah, we also have this append mode versus template mode. Append mode is how I started off this project — we're just always appending the responses to the bottom of the document. But with template mode, you can actually set different components within a document, and you can section off where you want things. So this whole area is in an exchange component. Here's the beginning of the exchange, looks like we have a history component over here. And yeah, so here's our exchange section, this is the bottom of it.

This is kind of meant to be like, okay, you get the response and you can also put inline replies or inline prompts within the response area. One thing I also want to mention about template mode is I am going to support real-time feeds as well. So this will be like a true dashboard in a document format — we can have full-on real-time updates going on. But I'm not quite there yet, still need to get the CRDT nailed down and probably tighten up some other things.

Right now there's like a 200 millisecond poll, which I'm not super happy with. I mean, I guess it's okay — maybe it is the right way to poll 200 milliseconds. But I would like it to be fully reactive personally. That's kind of what I feel like I'm going for. I guess as long as it's not too frequent — maybe if it's super frequent we should poll, but for the most part, yeah, it probably should just be reactive. But 200 milliseconds is a good enough approximation for now for the poll, so we'll just run with that. Claude really wanted us to poll — whatever, because it'd be too complicated. But yeah, okay, whatever. We'll make it reactive soon.

So anyways, yeah, the other issue that's been really nagging me — like for I don't know how many releases I've attempted to fix this problem — we have a situation when there's a document that has a stale claim on it, meaning that the document was opened in the previous session, and then we try to call run on it. It then gives two problems. The first thing is when you call run, it doesn't update the TMUX pane with the Claude session that is actually running that command. And the second problem is it didn't actually run the command.

So basically what I did was I copied the entirety of this pane over here. And the other thing is I also added stream mode recently. Stream mode's a little different because with stream mode, we need one Claude session per document — that's it. With template mode or append mode, which are basically both batch, you could multiplex multiple documents in a single Claude session. But with stream mode, that'll be cleaned up to where it'll be one document per Claude session.

But anyways, we didn't run what should have been called — Agent Doc run tasks/gmail.md. This is what should have happened, it should have just popped in right over here, but that didn't happen. And basically the reason why is because of the bug where gmail was stale — all this front matter, this session was wrong. And I'm not sure exactly what the issue is, because yeah, I'm a vibe coder, and I don't really know what's going on. So I gotta actually manually debug this thing, because something is off. Claude's assisting me in manually debugging.

I'm paying a little bit more attention to the source code now, because it wasn't a quick fix — Claude couldn't figure it out on its own.

Here's the other thing I've been having — "your message was cut off." This happens every once in a while. It did the diff too early, so I'm hoping to nudge it to have a heuristic where if it notices a cutoff like this, it'll maybe try again or something like that.

So you can see that in this document, I've had a multitude of different issues, and what is kind of cool about this is I can start to fix them all, more or less, in single passes. So we can actually parallelize a little bit — or asynchronous, I guess — our efforts based off of fairly complicated issues.

And how effective is it? Obviously, in the case of some of these reoccurring issues, like the stale claim auto-claim, hasn't been effective yet. But now I'm starting to dig in a little bit deeper. Basically the strategy I'm gonna do is start adding more logging, and also just start to go through a process of elimination — also simplification of the path. The path of run, Agent Doc run, was a little complicated. They had some duplicate code, so we removed that in the previous version that we released, and we're also simplifying the path a little bit.

So anyways, okay, it looks like I found a root cause for the CRDT base. They got some logic here — if ours aren't matching, it picks one. And I guess I should probably take a peek at it personally. I haven't really been peeking at it too much — all the diffs, whatever Claude does, I've just kind of been trusting Claude. But now things are starting to — I'm getting reoccurring issues. So I gotta probably step in a little bit more than I usually do.

For the most part, Claude's pretty good about making these changes and fixing things, or making sure things work properly. But in this case, it ain't quite cutting it.

So this is like really my first real debug where I had to use bookmarks, using Claude. Because normally Claude just handles this stuff. But obviously this is a pretty sophisticated problem we're hitting right now.

Here's one thing I am a little concerned about — we got some dead code here. It's used by recover to apply orphans. I should put doc comments on these. And actually, because I'm using Claude, I'm not super intimate with all this code and the entire feature set.

I think recover has to do with recovering from a stale state. Is recover used to mitigate a stale claim? And this is kind of where domain-driven development is super powerful — once we nail down this terminology and have precise terms that we are expressing to the LLM and also thinking and expressing in our own language, that's where it's very helpful to be precise.

### Existence Lang Detour

I'm actually working on a project called Existence Lang, which basically — we have an ontology. We just treat each term in the ontology as its own markdown file. And Existence Lang actually starts from a scope of all of existence. So we kind of treat a kernel of terms as this thing that we can work from, and then we can down-scope it — scope it, zoom in, scope it for projects or functions or basically whatever we want to talk about. We have some sort of implicit scope. Anyways, more to come on that — I'm still getting through Agent Doc.

So we kind of had this convergence of two different things for recover and run stream. I really wish I had a map. So — could you create a diagram of the call paths that converge to merge? And again, this is where Agent Doc is really good — this sort of deep inquiry and analysis. Because if I just kept running prompts one at a time, I would just be lost personally. Having a conversation and talking about five different things — the LLM will get confused. The nice thing about Agent Docs is we can kind of stop time and look at this really complicated set of problems and start to really analyze it.

Sometimes it's useful to treat all these problems together because there might be some systemic underlying issues that we can address. That's kind of why I like to work with the complexity in all of its Hydra glory. Because sometimes you can't break off each scale — you just got to deal with the heads. And maybe there's a branch of heads, two or three heads branching off from the same neck, and then you just cut off that one neck and boom, you got three heads gone.

### Truncation Detection

So Claude finished after five minutes. It looks like I was ranting for that long. Thank you Claude for giving me the space.

So why append mode? We should be doing template mode. This is intentional. Oh, okay — Claude is disobeying me here. Should use template mode by default. In fact, this file uses template mode.

Here's what I propose. We currently have agent_doc_mode with two choices. But maybe I want to say we have two categories. Category one: stream and batch. Category two: template and append.

So this might be a big release, because now we're going to probably need to split up agent_doc_mode. We need to come up with good names for category one and category two. Agent_doc_mode was doing double duty for both categories. So please propose config names.

### The Power and Nightmare of Agent Doc

This is kind of the power of Agent Doc, or maybe the nightmare of Agent Doc — yes, you can have very complicated back and forths now. You cannot do this with just a single prompt at a time. This ain't going to work.

So we got the stale claim problem that we've been dealing with for probably almost 10 versions. I've attempted to have Claude fix this close to 10 times and it has not succeeded yet. Or it did succeed once, but it completely borked my environment where it would shut down my TMUX session entirely. So I had to revert that.

This is kind of where async might just help avoid all these weird corner cases. The route path was a little complicated with some duplicate code. Should this be async? Should async to handle the race conditions? Can we have the async loop block until each step completes — the TMUX pane starting, being attached to the correct TMUX window position, the Claude session confirmed running and available?

### Existence, God, and Philosophy

So yeah — SIFT is a cool project. Shout out to Alex for making SIFT. It's like a search tool that creates vectors and it's super fast. I want to use SIFT and I think there's a lot of good integrations I can do with Tag Path. Tag Path is a library where you can take all these different code styles — snake case, Pascal case, whatever — and it'll take each segment of the name and turn it into a one-dimensional vector. From it you can get semantic meaning. From a really obvious standpoint, I can just type in "user space ID" and Tag Path will use tree-sitter to go across your JavaScript, Python, or Rust codebase and find all the different styles. And you can configure it too — you don't have to follow the canonical style for a programming language.

And of course, Agent Doc and Corky. Basically, fast semantic search integrated with the LLM would be amazing once I get some bandwidth.

And yeah, I'm one of those guys who uses LLM to craft my responses because I'm not a very good writer. My prose is a little too punchy for a lot of people. Claude really helps me express myself in a way that is a little more palatable. I'm prosaically challenged and charismatically challenged perhaps, but Claude's got my back.

I know Hacker News says you can't use the LLM to talk on the forums because they want to hear the real you. But Hacker News does not want to hear the real me — they downvote me when I express myself. So I think I'm doing everybody a favor by using Claude more when I talk online.

I used to be a very nice guy. I used to measure all my speech and be extremely polite, but I found that people just walked all over me. And I haven't really discovered the middle ground yet. Everybody's got their challenges and this has been mine. Actually, communication is something I got into philosophy for — being able to express intent. Maybe this might actually be a blessing in disguise and we'll see with Existence Lang.

### Configuration Naming

So Claude finished after five minutes. Category one and category two — good analysis. Agent Doc write and Agent Doc format. Alias write and format. I like to do explicit scoping — this is part of my whole philosophy. So agent_doc_write: stream, and agent_doc_format: template. But I generally prefer explicit scoping because it's composable and will just work in more circumstances.

### Speculative Execution and Debounce

I'm now wondering whether or not I like debouncing. 500 millisecond debounce adds latency. What if we reduced it to 200 milliseconds?

With debounce, I don't want to add latency. Can we have the 500 millisecond debounce with optimistic concurrency? Meaning we speculatively execute before the debounce. If there's a debounce hit, cancel execution. If there's a debounce miss, continue execution, rerun with new diff.

Manual submit has no race. Agent Doc is explicit — whatever's in the file is intentional.

### Reactive Stream Mode (No Debounce)

Claude proposed reactive stream mode, no debounce. Debounce is a pre-CRDT protection. In stream mode, CRDT already handles concurrent edits. So: first file change goes to process immediately — zero latency. User keeps adding — CRDT merge handles it in the next flush. No debounce, no cancellation, no waste of tokens.

How does the next flush get folded into the agent loop? Please make a diagram.

Each 200 millisecond flush reads the current file with user edits. CRDT merge is agent output plus user edits against the baseline — writes merge result atomically. Stream flushes trigger file changes, but existing loop prevention — coverage detection plus cycle counter — prevents re-triggering during active streams.

### God = Existence = The Universal Set

So God — I know we got atheists in the audience, but by definition, God exists. Sorry atheists, but your view of existence also exists too. So it's all good. We don't have an existential crisis here. You're represented too, but your definition of God is implicitly scoped and is not this definition of God here, which is the universal set.

And God is existence too, because this is part of the universal set. God is also encompassing of existence. So that's kind of what I figured — how I got around "what God is" and proving God exists: God is existence, and existence exists. Just like the universal set is part of the universal set. Yeah, we got that recursion thing going on.

My whole premise was existence as God. What existence means is not constrained to "does it physically exist" or "is this a real thing that has happened." No — real is different from existence. Existence is a concept. The tooth fairy exists in this nomenclature because it's a concept we're talking about. Just because it's an idea, it exists. It's like software — does code exist? Yes, it does, in my opinion.

The reason why I have this philosophy is because I want to be able to describe anything in existence. You got to start with the whole big picture — the universal set of everything. That's what existence is. Except it also includes itself too — the universal set is part of existence. So it's one of those recursive things. It's not this neatly encapsulated concept — it's very messy, very recursive, and it also includes the unknown unknowns too. Things that have never been thought of before and maybe will never be thought of — that also exists. It's sort of this conceptual "everything that could possibly exist" — that's just part of existence. It's a very broad term, and it's meant to be the universal set.

Then I do scoping — explicit scopes and implicit scopes. Explicit scopes: I define what the scope is. The idea is that all language in an ontology is a pattern language. Pattern expression. And implicit scoping is like when you talk to people — "yeah, that doesn't exist." They really mean "the tooth fairy doesn't exist" — in physical existence, in our shared experience. But that is an implicitly scoped meaning of existence.

And this is kind of more of a Gnostic, panpsychist, pantheist thing. I've definitely always kind of been in that camp. I really want to be in the fold of Christianity, but I feel like Jesus was different from the institution of the church. The church kind of co-opted things — it's more of an Imperial mechanism. And Jesus, I don't think, was really about empire so much. But nobody's perfect. No system is ever really perfect. We just do the best we can. Hopefully we can improve and we are humble enough to improve.

### Existence Lang

So Existence Lang — I got a website. It's got a kernel, a 14-term kernel. This is definitely in flux — I'm not really sure yet about this kernel, but we got something. Supposedly this kernel is all that we need to define any system, any scope, any contextual system with the same linguistic schema that we can then apply. We got ring zero or kernel, ring one or software, ring two extended. I'm not really sure about these rings — something Claude recommended. I'm still thinking through this.

I'm kind of thinking of this more like: schema with context creates meaning. I feel like there can probably be set theory applied to a lot of this too, and some sort of formal methods. But right now I'm not quite that experienced — I didn't go to grad school. I just got into programming, agile methodology, domain-driven design. And I was like, yeah, it's a pretty good way of solving problems — having the ubiquitous language. But with the rise of LLMs and Claude Code and Codex, I can turn this into executable code.

### Release v0.16.0

All right, let's release. Let's do it live. It pretty much always works — I got my tests. Yes, I know Claude can cheat and sometimes Claude's sneaky, but I'm usually like, yeah, don't screw with me Claude. I've only had to scold Claude once really.

Agent Doc does support sinking chain of thought, but I just don't do it yet. Maybe I don't want TMUX on the bottom — maybe I want the full view for my editor and want to see what's going on in real-time. The idea is there's going to be another component with all the chain of thought — a log component with all the chain of thought stuff, and I can just go down when it's done.

All right, I think we got v0.16.0 coming. This was a good-size release.

### The Stale Claim Bug Persists

So we have Agent Doc compact... and it didn't work again. Oh my God. This bug is so elusive. I guess I'm gonna have to go for another round of trying to fix this problem. It didn't refresh this pane at all.

Now you can see how much of a noob, how much of a vibey Rust guy I am — I don't even know how to install a crate.

All right, so path shadowing — cargo release. I had to add to my CLAUDE.local.md to always run cargo build --release when releasing Agent Doc.

This is just my view of things and it just happens to work because I have utility for this. This is really utilitarian. I'm using this to represent any concept. And yes, the tooth fairy is a concept that exists. And so is Santa Claus and all these things. I have to be able to represent that.

If you want to fork the project, do your own thing — that's what I'm talking about. Please fork my code. Or if you want to give me some attribution, I could really use it. I could use the reputation, the money, the fame, the power. Right now I'm in between jobs. I'm just trying to stay relevant.

But no, this is a good hype train to be on because this is the culmination of over 10 years of desire. It's actually been very therapeutic for me. It's more than just hype — this is the next step.

Yeah, I would appreciate whatever — however you want to see it. I want to hear from you. I love to have twin flames or kindred spirits here. Maybe it resonates with you too, or maybe you just want to check it out or comment. If you don't like it and you're critical of it, I love to hear it too. Anything helps because it's better than just being ignored.

But anyways, we got here and I think we're good. So hopefully in the next session, we can really figure this out.

Anyways, I'm going to hop off now. Got my buddy calling me. This is a good stopping point. So hopefully in the next session, we can really figure this out. I guess I'll put this up on YouTube — this is my first video. Yeah. Ciao. Have a great day. Thank you for tuning in. Like and subscribe. Peace.

Please add a closed captioned file with timestamps in /home/brian/work/btakita/agent-loop/mail/topics/brian-takita/videos

<!-- /agent:exchange -->
