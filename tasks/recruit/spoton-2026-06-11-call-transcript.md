# SpotOn call transcript - 2026-06-11

- Source recording: `../../../../../Videos/2026-06-11 12-31-50-compressed.mp4`
- Duration: 40:56
- Transcription: local OpenAI Whisper `turbo` model, no external audio API used
- Diarization: local speaker embeddings clustered into 3 speakers, then mapped using the Zoom labels and spoken introductions
- Caveat: this is an automated rough diarization. Short back-and-forth moments may be combined into one speaker line, especially near speaker changes.

## Contacts

| Name | Link | Notes |
| --- | --- | --- |
| Rafael Alonso | https://www.linkedin.com/in/rafaalonso/ | Engineering manager at SpotOn; based in Poland during the call. |
| Mike Kai | https://www.linkedin.com/in/mikekai/ | Product owner at SpotOn; based in Los Angeles during the call. |

## Speaker Map

- Cluster 0: Mike Kai
- Cluster 1: Rafael Alonso
- Cluster 2: Brian Takita

## Call Summary

- Mike introduced SpotOn onboarding work: merchant signup, merchant applications, KYC/underwriting, Salesforce handoffs, and implementation after a merchant signs.
- Rafael described a small engineering team that leaned heavily on AI tooling after layoffs and works from high-level requirements with broad ownership.
- Brian summarized his background from Pivotal Labs, startup/client work, AI-assisted agent-doc workflows, and a preference for customer-facing product work.
- The group discussed how SpotOn talks with customers during selling and implementation, where restaurant POS installs can lose momentum, and how onboarding/installation can become more self-serve.

## Diarized Transcript

[00:00-00:09] Rafael Alonso: well myself thank you um where are you calling us from i'm in connecticut um okay all right are you
[00:09-00:18] Brian Takita: in poland and you're in la mike yeah i'm in la cool yeah whereabouts in connecticut uh i'm in um
[00:18-00:24] Brian Takita: near thompson so that's northeastern connecticut right by the border of rhode island and
[00:24-00:29] Brian Takita: massachusetts oh cool i used to live in la and actually i'm thinking about moving back
[00:29-00:40] Mike Kai: so oh okay right on awesome i uh i did my undergrad at yale in new haven and i drove up and down the
[00:40-00:46] Mike Kai: coast a bunch to get to rhode island back in the day so i know that now they know that area it's
[00:46-00:53] Brian Takita: so beautiful especially in the fall yeah yeah the beaches are nice now though i mean not not quite
[00:53-00:57] Brian Takita: la but they're at least kind of empty though actually some of the beaches oh yeah i think
[00:57-01:02] Mike Kai: they're better yeah la is it's kind of all the same you know but there's it's just like new
[01:02-01:08] Mike Kai: england's so unique you know wherever you look the seasons etc um well brian thanks for joining us here
[01:08-01:14] Mike Kai: we're excited to just have a meet and greet um i could just we could just do quick introductions on
[01:14-01:19] Mike Kai: our side and uh explain you know the products that we work on would that be helpful to start yeah that'd
[01:19-01:28] Mike Kai: be great okay cool so um yeah i am product owner for uh the apps that we manage we are focused in
[01:28-01:35] Mike Kai: the area of spot on that's all about a merchant that joins us to uh you know make their purchase
[01:35-01:43] Mike Kai: decision and then we install their point of sale so we handle the um front end for the sign up flow
[01:43-01:49] Mike Kai: uh there's like a merchant application process a bit of kyc a bit of underwriting um and then we do
[01:49-01:53] Mike Kai: handoffs to salesforce and we're increasingly getting involved in the salesforce world where
[01:53-02:00] Mike Kai: the business owner goes through an implementation process um the the key challenge that we have now
[02:00-02:08] Mike Kai: is that implementation process that exists with spot on today um on one hand we say it's white glove
[02:08-02:12] Mike Kai: which is a really nice way of saying to the business owner that we're gonna you know do
[02:12-02:17] Mike Kai: everything for you um but the challenge that we have internally is that white glove also means
[02:17-02:24] Mike Kai: highly manual and touches many different people and different work streams and as we've scaled we've
[02:24-02:31] Mike Kai: thrown more resources at it but we haven't sort of had a product-led approach to make that onboarding
[02:31-02:36] Mike Kai: and implementation experience um what we're calling internally like tesla like so you know if you ever
[02:36-02:43] Mike Kai: bought a car on tesla or carvana you don't have to talk to somebody all the steps that you need to
[02:43-02:48] Mike Kai: do are really clear in terms of providing information where you need to be you know to pick up your car
[02:48-02:55] Mike Kai: uh checklist that sort of thing um we're trying to build that but for the point of sale install and
[02:55-03:01] Mike Kai: it touches a lot of different areas you know a menu build an installation and all the resources that
[03:01-03:10] Mike Kai: are behind that um one more ad my background i went spot on like seven years ago i had a diy website
[03:10-03:16] Mike Kai: builder platform that um was acquired by spot on and i ran that product to spot on for half of my
[03:16-03:22] Mike Kai: tenure and then rafa and i teamed up and we've been working all about the onboarding you know sign
[03:22-03:30] Mike Kai: and sign experience um since then um two kids four year old and nine year old uh so that's a big big
[03:30-03:40] Rafael Alonso: part of my job as well to say at least yeah um hey i'm rafael alonso i have been at spot on for about
[03:41-03:48] Rafael Alonso: six years now i i joined spot on about a year later uh when we were launching a d when we were starting
[03:48-03:55] Rafael Alonso: a different product called retail which we ended up killing anyway uh so mike and i i have something to
[03:55-04:06] Rafael Alonso: bond uh uh about uh the death of uh of our babies uh and i but i have been an engineering manager
[04:06-04:15] Rafael Alonso: here for about four years now um working with mike for about two three three three mike right three years
[04:15-04:22] Rafael Alonso: um uh i am originally portuguese but i'm both to to poland
[04:22-04:33] Rafael Alonso: uh and uh the engineering team right now is small to give you a picture so the team about a couple
[04:33-04:41] Rafael Alonso: of years ago uh we were like seven engineers uh but we went through the round of layoffs that was going
[04:41-04:52] Rafael Alonso: around at the time during the the crash of it um we were reduced to three and uh at the time we i mean we
[04:52-05:00] Rafael Alonso: still had work to do um and this was when the the whole ai craze started um so we were looking for
[05:00-05:09] Rafael Alonso: uh ways to be more effective so we have been leaning heavily on on ai to to stay productive um and
[05:09-05:20] Rafael Alonso: and we've created a a culture that relies on ai uh here and that's and that's the culture you would be
[05:20-05:27] Rafael Alonso: going into so there is a certain level of uh of expectations around you know how uh how efficient are
[05:27-05:38] Rafael Alonso: you using these tools what level of uh independence um you have uh which is going to be a lot of it um
[05:39-05:51] Rafael Alonso: um yeah i think that that's uh uh that's it on my introduction i'll let you talk now brian we've
[05:51-05:59] Brian Takita: been doing all the talking oh okay uh sure yeah thank you um yeah i've been um brian been developing
[05:59-06:07] Brian Takita: professionally since 2002 i got my first big break at pivotal labs in san francisco um during the web 20 days
[06:07-06:15] Brian Takita: um we were really heavy on the agile process so we really so i would say um made you know
[06:16-06:21] Brian Takita: did a lot of um sort of brought the agile process into the software development ecosystem in the san
[06:21-06:30] Brian Takita: francisco um startup area um with clients like twitter google and ebay amongst others as well as many
[06:30-06:39] Brian Takita: startups i worked mainly with startups so um i actually left pivotal labs with a client um honk.com
[06:39-06:47] Brian Takita: which is an auto buying startup based in san francisco and we got acquired by truecar in in santa monica
[06:47-06:54] Brian Takita: and did some work at truecar and then this was pre pub right before they went public and then um
[06:54-07:02] Brian Takita: they went public um i i also so i left truecar i went to sort of do my own practice and i was really
[07:02-07:09] Brian Takita: wanting to explore um areas of domain driven design and um i actually have um an interesting
[07:09-07:16] Brian Takita: a couple interesting projects that are related so one i did a um document interface to salesforce
[07:16-07:22] Brian Takita: so where hedge fund analysts could then you know interact with salesforce as a document so it allow
[07:22-07:27] Brian Takita: you to type in commands into the document kind of like a dsl like a domain specific language almost
[07:28-07:34] Brian Takita: where they could then sort of drive the interface to salesforce by doing queries as well as entering in
[07:34-07:40] Brian Takita: you know or updating up certain records into salesforce from the document so it was just like a sort
[07:40-07:46] Brian Takita: of a free text way of um sort of getting notes about interactions with different clients or doing
[07:46-07:54] Brian Takita: research on companies and then you know seeing what they had in their um in in their salesforce and as
[07:54-08:04] Brian Takita: well as adding new fields um and then so kind of that relates to now where i have a project called
[08:04-08:13] Brian Takita: agent doc which is a um ai um interface so it uses um markdown documents as the primary as a real-time
[08:13-08:21] Brian Takita: interface to agents so it's integrated with cloud code open code and codex and i can you know i i
[08:21-08:28] Brian Takita: basically have all the workflow within a document so i can enter in prompts i can queue up prompts into
[08:28-08:34] Brian Takita: like you know list a queue list and i can also edit while turns are going and then
[08:34-08:40] Brian Takita: it'll pick up the changes um and that's sort of what i use to sort of drive my you know interface
[08:41-08:46] Brian Takita: really in rust and i have like a number of tools in rust as well which include like saving tokens
[08:46-08:54] Brian Takita: mem graph rag and um also like a prompt cache just to because i use up a lot of tokens so i got it for
[08:54-09:01] Brian Takita: various projects um when i actually worked on a related project um was was a startup in la called menu.com
[09:02-09:09] Brian Takita: so they served restaurants and um sort of was like you know like a like a website but also a back-end
[09:09-09:14] Brian Takita: for restaurants and they didn't quite get to point of sale but you know they were sort of flirting with
[09:14-09:22] Brian Takita: it a lot but um yeah it was just sort of uh oh so sort of the the main re the main um draw menu.com was
[09:22-09:31] Brian Takita: sort of um people could look up like individual like like what they want in like a for a meal so
[09:31-09:36] Brian Takita: like i want like you know falafels or something and show me all the restaurants in the area with
[09:36-09:43] Brian Takita: falafels and then they could also sort of batch up different um you know menu idea like different
[09:43-09:47] Brian Takita: meals as well if they you know there's like multiple people who want different things and it could then
[09:47-09:53] Brian Takita: sort of create an order for them based off of that and then also do they handle delivery too so um
[09:54-10:03] Brian Takita: and um yeah lately i've been working you know i worked at presence.ai which um i was responsible
[10:03-10:09] Brian Takita: for a real-time video agent so this was like a lip syncing agent so this was a lot of python work we
[10:09-10:16] Brian Takita: took an open source model called latent sync which um we ran in our own gpus and we used live kit for
[10:16-10:21] Brian Takita: the rest of the pipeline the voice the real-time voice to voice pipeline and then we would take the
[10:21-10:26] Brian Takita: the output voice and then do lip syncing off that so i was responsible for making the lip syncing real
[10:26-10:33] Brian Takita: time to fit within the real-time budget um so i had to do some performance work with pi torch and numpy
[10:33-10:39] Brian Takita: as well as you know some cuda work and also had to um was also responsible for the pipeline the build
[10:39-10:45] Brian Takita: pipeline so for all of our avatars so this was um for the admin admins would create like
[10:45-10:50] Brian Takita: thousands of avatars and users could create their own avatars too um presence went out of business
[10:50-10:56] Brian Takita: because it wasn't really viable and um so in february so since then i've been you know i started
[10:56-11:02] Brian Takita: working with agent doc as well as my other open source tools for agent tech development and i have
[11:02-11:10] Brian Takita: a few like you know clients right now um i worked for a stealth startup where we did voice-to-voice as
[11:10-11:18] Brian Takita: well as rendering um i would say assets based off of that um some of the assets were real-time so it's
[11:18-11:22] Brian Takita: basically presentations we could give with with the avatar and then some of the things we could do real
[11:22-11:28] Brian Takita: time but some things had to be you know it would take some time to generate um while you know the
[11:28-11:34] Brian Takita: avatar is talking but then we could present it afterwards like and then also you know these you know
[11:34-11:41] Brian Takita: we use curse uh co-pilot kit to um help drive you know and make the the the ui is more interactive so
[11:41-11:49] Brian Takita: the user could interact with it or the the agent could also you know create um like animations or or
[11:49-11:55] Brian Takita: you know visualization could drive the visualization based off of using copilot kit um i also have like
[11:55-12:05] Brian Takita: a side project right now with uh that i'm outsourcing to um which is uh um commercial real estate portal
[12:05-12:12] Brian Takita: so it's a whole like series of portals for for lenders borrowers and brokers as well as administering
[12:12-12:21] Brian Takita: that and that's just more business workflow um and work you know automation type stuff and also i'm
[12:21-12:28] Brian Takita: working on an e-commerce site for my friend who's does he has some contract manufacturing so he sold
[12:29-12:33] Brian Takita: it's called monster rod holder so it's fishing rod holders so we sold them a bunch of fishing rod holders
[12:33-12:38] Brian Takita: so i'm helping one of his clients to so he can sell his product basically and get sort of my friend
[12:38-12:46] Brian Takita: can get paid so that's um yeah for for his order but yeah i'm so i'm kind of dabbling in a lot of
[12:46-12:52] Rafael Alonso: different areas but um yeah in between all of that when do you sleep um you know a lot of it is
[12:52-12:58] Brian Takita: automated but yeah you know i i do work a lot right now um just trying to get as much as i can get done
[12:58-13:06] Brian Takita: but um yeah you know like for example when i took on the stealth startup client that was a priority and
[13:06-13:12] Brian Takita: i worked actually a lot of hours on that that was a pretty grueling job but um you know i i can
[13:12-13:18] Brian Takita: prioritize my different work but yeah i i have my schedule um i you know work from home so i have a
[13:18-13:28] Brian Takita: five-year-old daughter too and we um you know we we make do and cool uh i mean there are that that's a
[13:28-13:40] Rafael Alonso: great age um what are you looking for uh to take out a spot on you know i i like the i decided to
[13:40-13:46] Brian Takita: change go ahead i do i do like the idea that you you're working with customers directly um you
[13:46-13:51] Brian Takita: know with with restaurants and your white glove service sounds really you know really cool like
[13:51-13:59] Brian Takita: i want to learn more about like working with you know actual brick and mortar or just you know
[13:59-14:05] Brian Takita: smaller businesses um that's because that really aligns a lot with you know what i have been doing so
[14:05-14:11] Brian Takita: i'm very interested in how to make that work for you know for smaller companies and restaurants i
[14:11-14:18] Brian Takita: know the margins can be very tight and just making the operations efficient is and as well as bringing
[14:18-14:25] Brian Takita: in customers is you know extremely valuable so i think that just the you know and also i do like
[14:26-14:32] Brian Takita: you know restaurants you know i believe in the dream of um of people owning their restaurant and they
[14:32-14:37] Brian Takita: put a lot of creativity and a lot of their soul into it so just helping them succeed is very you
[14:37-14:46] Brian Takita: know gratis would be very gratifying all right on brian yeah it's really exciting to hear your background
[14:46-14:54] Mike Kai: um i i too before spot on uh to keep the lights on on my startup i work for a small team building apps
[14:54-15:01] Mike Kai: for startups um so i definitely know the grind of like you know just re like getting very quickly deep
[15:01-15:08] Mike Kai: in like a workflow or use case and then building it delivering it um the highs and lows right of like
[15:08-15:14] Mike Kai: project-based work um i mean for me and myself i'm just curious your your thoughts on this like it was
[15:14-15:19] Mike Kai: always really engaging because you get the cycles you know and you see the same patterns whether it's
[15:19-15:26] Mike Kai: like onboarding or um various parts of the user flow uh one of the challenges i've had it spot on we've
[15:26-15:31] Mike Kai: kind of solved this because now we have too much work uh is you know at a larger company you're more
[15:31-15:39] Mike Kai: focused on like more of a silo product and you don't get to use those cycles as much um but i guess
[15:39-15:46] Mike Kai: quick question for you brian uh building on rafa's point and i'll be i guess kind of pointed on this like
[15:46-15:52] Mike Kai: if you were to join spot on how would your like world be different in terms of all these side projects
[15:52-15:58] Mike Kai: that you have vendor clients um do you have like sort of a thought or a plan in place for that yeah
[15:58-16:05] Brian Takita: because you know like for example the monster rod holder thing is really a long-term project and i've
[16:05-16:12] Brian Takita: already put in some work you know and i have time right now to put in extra work into it but um you
[16:12-16:19] Brian Takita: know i'm not like you know obviously if i were to do a full-time job it's it's gonna be not as much
[16:20-16:24] Brian Takita: time into that but you know i've been working a lot on sort of preparing and automating as much as
[16:24-16:30] Brian Takita: i can as well as you know like you know i've been learning about ads school ads you know seo
[16:30-16:36] Brian Takita: creating the content for seo and all that so a lot of this stuff is upfront work that i'm doing now
[16:36-16:42] Brian Takita: and then you know i plan on just sort of i wouldn't say coasting but you know like you know sort of
[16:42-16:48] Brian Takita: enjoying the fruits of my work i'm doing right now um and then with the you know equity funding source
[16:48-16:53] Brian Takita: which is the commercial real estate um i don't know if you have that with my resume but um that
[16:53-16:58] Brian Takita: that's i have that outsourced so i'm you know primarily you know we're getting the initial
[16:58-17:07] Brian Takita: launch out and then um you know we're pretty we're sort of completing that right now so i think that um
[17:07-17:14] Brian Takita: this is actually a great time to to take on the next thing and brian if i capture your experience here
[17:14-17:20] Mike Kai: like do you do you consider yourself like a very like multi-disciplinary like like developer like
[17:20-17:26] Mike Kai: it sounds like you've had to take on various numerous use cases and you've worked as like
[17:26-17:33] Mike Kai: you know individual stakeholder like small teams um or do you have like a specific like kind of
[17:33-17:40] Mike Kai: workflow focus uh that that you that you've had in your in your career sorry i'm butchering this
[17:40-17:49] Brian Takita: question but maybe you can capture the context oh yeah yeah um yeah so let me see if i can yeah
[17:49-17:54] Mike Kai: like i see like your experience like using figma and whatnot so i what i see is like you weren't just
[17:54-17:59] Mike Kai: you know taking tickets and implementing you sort of had to look at the broader picture of each
[17:59-18:05] Brian Takita: these individual products in projects yeah i definitely aspire to you know wear whatever hats i have to
[18:05-18:12] Brian Takita: like oftentimes if i'm part of a team there's often like i would say i won't say gaps but you know
[18:12-18:19] Brian Takita: areas where i can fill in and that's that's so i guess yeah filling in the gaps so that's kind of how
[18:19-18:26] Brian Takita: i usually approach like being on a team but yeah i think that that's where sort of the broad range really
[18:26-18:32] Brian Takita: helps is just being able to do different things and also yeah i think that ultimately like long term
[18:32-18:40] Brian Takita: it is valuable to you know have um the broad experience because um you know that's kind of
[18:40-18:48] Brian Takita: what you know to find solutions it's good to not really be constrained to a particular little box you
[18:48-18:54] Brian Takita: know kind of thing so i think that that's sort of what i'm you know i do aspire to handle whatever i can
[18:54-19:00] Brian Takita: do but you know i can specialize like for example with presence i specialized in in the real time you
[19:00-19:04] Brian Takita: know it was deeply technical and also the open source work i'm doing right now is pretty deeply
[19:04-19:12] Brian Takita: technical too so but yeah i do also like the more the less you know more more marketing focused work
[19:12-19:22] Brian Takita: and i'm i'm really enjoying whenever i get a chance to do that as well um yeah to to expand on mike the
[19:22-19:32] Rafael Alonso: the way that right now we're uh we're running the team is that um we get uh very high level requirements
[19:32-19:41] Rafael Alonso: not even necessarily would that would not even necessarily qualify as a ticket um and then the
[19:41-19:47] Rafael Alonso: developers are uh responsible for taking it end to end and since the team is small the way that we have
[19:47-19:55] Rafael Alonso: been uh been running it it's that the uh developers own certain parts of the system there's knowledge
[19:55-20:03] Rafael Alonso: transfer but there uh when the parts of the system are their responsibility to make sure that uh they
[20:04-20:12] Rafael Alonso: they're up kept um and that's where you would be thrown into you you'll be uh uh you'll be responsible
[20:12-20:19] Rafael Alonso: for figuring it out you're gonna have support uh there are a lot of our people uh around the company
[20:19-20:29] Rafael Alonso: that uh that will help um fill your knowledge gaps um but it would be your responsibility make sure it
[20:29-20:37] Brian Takita: gets to the finish line okay yeah that sounds good that makes sense i yeah i like the ownership angle
[20:37-20:44] Brian Takita: that's definitely uh very motivating to have ownership over a part of the system and to excel at it for
[20:44-20:51] Mike Kai: sure yeah yeah it's exciting times i'm you know personally and brian like it's crazy to me just with
[20:51-20:58] Mike Kai: this ai stuff that we can do so much now like there are features that took me uh you know three months
[20:58-21:05] Mike Kai: to build back in the day for my website builder platform that is now just one prompt one shot you know
[21:05-21:11] Mike Kai: and it i don't tell us about how you feel about using ai because it i get a sense that you know
[21:11-21:15] Mike Kai: as you mentioned with automation and whatnot you're able to take on more um but yeah just let me get
[21:15-21:23] Brian Takita: your your pov i've like called the past few years yeah yeah i really like ai i i feel like it's a really
[21:23-21:31] Brian Takita: it really you know i like the workflow of it you know it's it feels like you know is you know and um
[21:31-21:37] Brian Takita: yeah it is great when one shot happens i you know i i find that especially when there's integration
[21:37-21:42] Brian Takita: that usually it's very iterative but even the iterative parts are very fast and just trying
[21:42-21:47] Brian Takita: to get into a good loop you know with you know whether it's an automated loop or you know just a
[21:47-21:53] Brian Takita: workflow loop you know i think that that it's nice that it's all sped up and you know you can kind
[21:53-22:00] Brian Takita: of see full features implemented you know in in hours when it could could have taken days before
[22:00-22:06] Brian Takita: and yeah sometimes you you know a lot of things are one shot which is great too and but but also um i
[22:06-22:13] Brian Takita: think that like it's important to set up you know the good harness and to make sure that um everything is
[22:13-22:19] Brian Takita: you know spec'd out um because you know you you know you want to guide the agent in a in a in a good
[22:19-22:27] Brian Takita: you know a good direction so it so its output is of high quality and you know is able to um sort of
[22:28-22:35] Brian Takita: be maintainable in the long term and i i also like that you know refactoring or like a lot of
[22:35-22:42] Brian Takita: the things that were like we should have done before but you know like screen proper specs but
[22:42-22:48] Brian Takita: you know just kind of or comments or you know stuff like that like kind of fell by the wayside but now
[22:48-22:54] Brian Takita: you know documentation but now a lot of this is really you know easy i wouldn't say easy to do but
[22:54-23:01] Brian Takita: is very tractable and can be automated or put into the process and i like that aspect of it so i feel
[23:01-23:09] Brian Takita: like it's a really good way to um create well designed and engineered solutions um and a lot of the the
[23:09-23:14] Brian Takita: sort of the work that was once yeah passed off because it was just too difficult and there was schedule
[23:14-23:19] Brian Takita: pressure can now be done and that i feel like from a long-term maintainability perspective it
[23:19-23:25] Rafael Alonso: actually does help speaking of which i i do have one one final question and then we'll let you ask
[23:25-23:32] Rafael Alonso: questions um what does your developer environment uh look like yeah so i've been developing agent doc
[23:32-23:41] Brian Takita: which is my custom environment right now and i have markdown files and what i do is they're they're like
[23:41-23:45] Brian Takita: into different like i would say elements kind of like you know i use like commented out elements
[23:45-23:52] Brian Takita: like almost like html elements and there's different sections so i'm like an exchange area which is where
[23:52-23:58] Brian Takita: sort of has my interactions so it'll have my i can put prompts in there and get responses in there and i
[23:58-24:04] Brian Takita: can also inline respond to the responses too so and it'll understand contextually but i've mainly been
[24:04-24:10] Brian Takita: working with a queue a work queue lately so that allows me to queue up all my work into different
[24:10-24:16] Brian Takita: tasks for the for different turns of the agent and i can also edit it as i sort of like you know am
[24:16-24:22] Brian Takita: the reviewing or designing things so i can sort of keep the work going as i'm sort of designing too
[24:22-24:28] Brian Takita: um and i can be really iterative about that um and i also have like a little like pin like a pending
[24:28-24:35] Brian Takita: list area where like i'm sorry a backlog area which is where i have like tasks and i can have my agent
[24:35-24:41] Brian Takita: also sort of schedule you know what the back you know what goes in the backlog like you can look in
[24:41-24:48] Brian Takita: the logs or for errors for bugs you know if i want to do like a design or like i have a plan i can break
[24:48-24:56] Brian Takita: it out into steps you know and sort of also interact with the steps um and yeah you know and i i use you
[24:56-25:01] Brian Takita: know different so i can work also in parallel too with different tasks at the same time very
[25:01-25:08] Brian Takita: effectively because you know it's almost just like working with code but it's you know working with
[25:08-25:14] Brian Takita: basically real-time markdown files which allow me to sort of interact with it like it's code but
[25:14-25:20] Brian Takita: then i can then sort of like watch you know kind of keep track of all the different tasks that are
[25:20-25:29] Brian Takita: running um and i do some automation but not like as far as like loops but not not as much lately i
[25:29-25:35] Brian Takita: i try to fold it all into my turns right now just so i can like with agent doc just to kind of keep
[25:35-25:40] Brian Takita: really good oversight of it especially if i'm developing you know new solutions but as like
[25:40-25:47] Brian Takita: things become more i guess repetitive then that's where i can you know create like automations like
[25:47-25:53] Brian Takita: agent loops if i if that requires an agent but also like deterministic software is also very
[25:54-26:00] Brian Takita: important i feel like um because you know that that saves on tokens but is also reliable and
[26:00-26:05] Brian Takita: deterministic so i can create state and you know state machines or whatever and just like make sure
[26:05-26:11] Brian Takita: that everything is going to be working the same 100 of the time so you know also you know like good
[26:11-26:16] Brian Takita: architecture is important because that helps drive the agent to create better solutions and to be that are
[26:16-26:24] Brian Takita: conformant with the existing architecture and also you know spec files well laid out specs you know
[26:24-26:32] Brian Takita: prds and plans are good for review across the team um you know just having good you know also like good
[26:32-26:37] Brian Takita: curation of the content so rather than just generating a bunch of content making sure that you always take a
[26:37-26:45] Brian Takita: pass to clean it up to clean it up and consolidate you know um things and and all that and you know
[26:45-26:51] Brian Takita: skills are good you know basically just the basics but also just i'm driving it through a document
[26:51-27:02] Rafael Alonso: interface primarily yeah oh all right uh i'm i'm actually gonna try agent uh agent doc okay i'm looking at
[27:02-27:09] Rafael Alonso: uh at the uh at a github uh it looks interesting i don't know if it's if it would work for for uh
[27:09-27:17] Rafael Alonso: for me uh for my workflow um but i will definitely i will definitely try it out okay cool well i'm i'm
[27:17-27:24] Brian Takita: actually i i'm actually about two months into a big release um so i'm going to release that first and then
[27:24-27:30] Brian Takita: then i'll let you know because i want you to get a good first impression of it but yeah i mean it still works but
[27:30-27:38] Brian Takita: but but it's going to be better so okay yeah what's the url uh brian oh it's on a github um
[27:39-27:44] Brian Takita: well i have a crate for it but also i have a github i haven't made a page for you yeah it's on your
[27:44-27:51] Brian Takita: github okay yeah i got um i need to um i can send it to you but yeah i need to oh thank you thank you
[27:51-27:59] Brian Takita: rafael yeah i need to um make actual nice website with actual proper intro videos but you know i haven't
[27:59-28:07] Rafael Alonso: done that yet so nice oh we're almost out of time we can go a little over uh brian just to make sure
[28:07-28:15] Brian Takita: that all your questions are are answered sure um um yeah so so you're mentioning that you were automing
[28:15-28:20] Brian Takita: sort of the white glove treatment and print to a product does that involve actually talking to
[28:20-28:26] Brian Takita: customers or is that like restaurants or is that more just like you have people sort of already have
[28:26-28:33] Brian Takita: the requirements or what's what's that workflow look like yeah um so right now that's all we do
[28:33-28:38] Mike Kai: is talk to customers uh when we sell and then when we implement uh we will continue to do that on both
[28:38-28:45] Mike Kai: sides the selling and the implementation side um i think the biggest challenge you need to solve is
[28:46-28:50] Mike Kai: making it really clear to the business owner what's next and what's required from them
[28:50-28:56] Mike Kai: and then the challenge being it's never like a straight linear path right so it's you know someone
[28:56-29:03] Mike Kai: doesn't show up for a phone call or they need an additional information about their menu um all of
[29:03-29:09] Mike Kai: those edge cases need to have essentially automation behind them to continue to remind and make the request
[29:09-29:16] Mike Kai: top of mind for that person it could be a business owner could be a implementation agent um
[29:16-29:22] Mike Kai: um today our biggest challenge is that we're doing all that manual so we're you know reaching out
[29:22-29:30] Mike Kai: calling emailing texting and it's taking the time of individuals to do that and in addition we have
[29:30-29:35] Mike Kai: basically too many phone calls so we have four call it four primary phone calls that the person has to
[29:35-29:41] Mike Kai: show up for and for a taqueria owner like that's just too many you know maybe for fine dining it's it
[29:41-29:46] Mike Kai: works you know because they've got a lot of requirements a big team you know um different
[29:46-29:53] Mike Kai: stakeholders within their organization um so yeah trying to drive down a number of steps but create
[29:53-29:58] Mike Kai: more automation so that when we do have humans reach out they're higher intent and you know higher value
[29:58-30:06] Rafael Alonso: for those uh that time and resource and to be uh to be clear though when we we talk we talk to cost
[30:06-30:13] Rafael Alonso: customers is we are spot on we don't necessarily have a lot of interaction directly as in uh as
[30:13-30:19] Rafael Alonso: engineers um we interact though with uh uh with our implementation folks which for us would be the
[30:19-30:27] Rafael Alonso: middleman between us and the and the the sales people um i want to stress though that this is vital
[30:27-30:35] Rafael Alonso: the work that we're doing just yesterday we had uh we we were being pinged by the ceo
[30:36-30:42] Rafael Alonso: where is this higher where are we on the interview process are are we there yet we gotta we gotta
[30:42-30:48] Rafael Alonso: micro micromanage this process because it's the you know we need this asap yeah i'll give you one
[30:48-30:55] Mike Kai: more piece of context brian uh we are imagine we have a car dealership we're selling the cars not
[30:55-31:01] Mike Kai: everyone is coming to pick up the cars and every percentage point that we move up of that installed
[31:01-31:06] Mike Kai: point of sale has huge impact to the bottom line of the business uh not to mention we put on all the
[31:06-31:11] Mike Kai: effort we put in all the effort to sell the deal to implement it as far as we got but we have a lot
[31:11-31:18] Mike Kai: of fall off and i'll give you a rough number 70 eventually install and we lose 30 so that that
[31:18-31:24] Mike Kai: that's a meaty meaty bucket that we're trying to attack right right it's 30 of of people that i have
[31:24-31:32] Rafael Alonso: already signed the contract with us they just never processed so this is why it's very important
[31:32-31:39] Rafael Alonso: uh to uh to do the uh to remove a lot of this manual labor because this is where the fall off is
[31:39-31:43] Mike Kai: yeah it's a fun process right because it's not just a manual labor it's like a lot of this is like
[31:43-31:47] Mike Kai: how do you keep the deal top of mind so like selling a car someone's like pumped they're excited
[31:47-31:52] Mike Kai: they signed the deal they got the car but then like time passes and they start to get cold feet and
[31:52-31:56] Mike Kai: then we ask them to do things for us and then they're like i don't have time for this guys
[31:57-32:02] Mike Kai: i'm out even though you ran my credit you had a hard credit pull you may have pulled a deposit
[32:02-32:07] Mike Kai: you spent all my time you know telling me about your point of sale solution even though i've invested
[32:07-32:11] Mike Kai: that i i gotta leave now you know right that's that's the psychology that we're trying to attack
[32:12-32:16] Brian Takita: yeah i can see that with the restaurants because they're probably really busy and they got a lot of
[32:16-32:24] Brian Takita: things like on their on their plate but no pun intended but yeah so i think that that yeah so
[32:24-32:30] Brian Takita: just what what um how so what is the process like now and what what are sort where are people dropping
[32:30-32:39] Mike Kai: off or yeah um i can give you the high level so part of the drop off happens immediately during the
[32:39-32:45] Mike Kai: underwriting process so you know deals that don't get moved quickly that don't get that next follow-up touch
[32:46-32:53] Mike Kai: you know as we say time kills deals um other ways things fall off uh as i mentioned we have phone
[32:53-32:59] Mike Kai: calls and when you need to schedule time with people and you require you know people to show up four
[32:59-33:05] Mike Kai: times uh that could be really hard just to your point about an average restaurant owner and what they
[33:05-33:10] Mike Kai: have to do and how they have to stack rank their day right um so if we have a deal that gets stuck on
[33:10-33:16] Mike Kai: phone call number three suddenly our team is manually calling and texting you know x amount
[33:16-33:21] Mike Kai: of times and that person goes dark you know and then now that's a non-workable deal for us it might
[33:21-33:27] Mike Kai: take months days months weeks right to mark that one is canceled but effectively we lost it we've lost
[33:27-33:33] Mike Kai: the deal um so you know it's like fishing you mentioned your friend i'm a big fisherman like you get a
[33:33-33:39] Mike Kai: fish on the hook you gotta get the damn fish in the boat you know like you can't just wait around for
[33:39-33:47] Brian Takita: it you know um so that actually reminds me of one of my clients that that mortgage real estate the
[33:47-33:55] Brian Takita: the mortgage commercial real estate um uh originator client uh they yeah i don't get very many calls but
[33:55-33:59] Brian Takita: yeah it's like you just have to implement the whole thing you know basically and sort of show them
[33:59-34:04] Brian Takita: something and it's going to work or not work you know but you know i think that you know it'll work
[34:04-34:09] Brian Takita: for them but it's it's kind of like it feels like that and or it seems like that and it's like people
[34:09-34:13] Brian Takita: don't really want to spend time in meetings you know they just want to yeah have something that
[34:13-34:19] Brian Takita: works and it's easy that can integrate wealth um so i i guess so with your is it is it like your
[34:19-34:28] Brian Takita: or is there is there friction so they sign the doc the deal then they they pay the money
[34:28-34:34] Brian Takita: they have they paid yet or is it like uh and then uh that's also a challenge so so and we're likely
[34:34-34:40] Mike Kai: going to you know pull some different levers to change the dynamics of the deal um but up front
[34:40-34:44] Mike Kai: there's really not a lot of skin in the game you know okay other than the time that you spent as a
[34:44-34:50] Mike Kai: business owner like dealing with us uh other companies do put skin in the game so you know
[34:50-34:57] Mike Kai: upfront deposit if you don't install you lose it uh uh if you don't install by x date we start
[34:57-35:01] Mike Kai: charging you you know eighty dollars a month literally one of our competitors calls it an
[35:01-35:09] Mike Kai: inactivity fee um so there's ways to yeah do set up um a bit out of our realm we're trying to focus
[35:09-35:15] Mike Kai: more on just the core friction parts but likely there's gonna be some levers we pull to drive that
[35:15-35:21] Brian Takita: uh engagement so with the core friction there's like the they signed a deal it's installing software
[35:22-35:28] Brian Takita: then the point of sales like do you you ship them the point of sales or you have an installer come
[35:28-35:34] Mike Kai: in you schedule yeah that's all okay exactly it yeah and like we we say it it's like it's you know
[35:34-35:40] Mike Kai: trying to perform open heart surgery basically right so you know these restaurants they don't close
[35:40-35:46] Mike Kai: they're essentially always open you know seven days a week um we've got to come in uh rip out that old
[35:46-35:52] Mike Kai: machine put in a new one make sure everyone knows how to use it uh make sure when it's plugged in it
[35:52-35:58] Mike Kai: has connectivity that is configured properly with the right menu um you know outside of our team scope
[35:58-36:04] Mike Kai: is certainly all the functionality of that pos but inside the team scope is how can we make sure that
[36:04-36:09] Mike Kai: when nothing turns on it's it's like essentially perfect for the business owner um and we've done a
[36:09-36:13] Mike Kai: lot of work on the front end and back end i'll give you just an example one of our first ai tools
[36:14-36:19] Mike Kai: year and a half ago now a year ago uh we made it so you could take a picture of a menu and it would
[36:19-36:25] Mike Kai: build the us in a demo state and the sales people love it like they get to walk in show them like
[36:25-36:31] Mike Kai: their future you know show them this cool new ui and show them their actual menu you know on the
[36:31-36:37] Mike Kai: point of sale um that said that menu is maybe 80 of the way there you really have to go with a fine
[36:37-36:41] Mike Kai: tooth comb get it perfect because if you try to go live with that menu and operations like
[36:41-36:46] Mike Kai: like you know people throw their hands up in the air basically do you do like analytics on the
[36:46-36:53] Brian Takita: menus like and and help to make it more effective over time or oh that's a good question um yeah we
[36:53-36:59] Mike Kai: do have product we have like a ai product that sort of looks at your sales at a menu level also looks at
[36:59-37:05] Mike Kai: your costs um um that's outside of our realm but that's part of sort of like post activation you know
[37:05-37:11] Mike Kai: value uh analytics in terms of menu build not at the moment but there's some really cool stuff
[37:11-37:15] Mike Kai: happening there's like someone built a browser extension to look at the entire menu build and
[37:15-37:20] Mike Kai: run it through ai and then call out like oh this looks a little bit off like this wasn't configured
[37:20-37:25] Mike Kai: you know that sort of thing so that's that's interesting as well and and do you also so with
[37:25-37:34] Brian Takita: this installation do you also help with like social media and website as well or is it um we do have a
[37:34-37:39] Mike Kai: marketing platform that's that's outside of our team scope but as i mentioned i used to run the
[37:39-37:44] Mike Kai: like website product uh that we had this whole like you know build a website for you the platform
[37:44-37:51] Mike Kai: we use now is a third-party platform um but you know i think what what might touch our world is
[37:51-37:56] Mike Kai: we're very involved in the sales process and so we're trying to like activate um folks
[37:57-38:02] Mike Kai: okay the exact marcio is calling me right now and zoom i got a hop but ryan it's been amazing
[38:02-38:07] Mike Kai: yeah thank you we'll reach out to you we want to schedule the next round as soon as possible
[38:07-38:12] Brian Takita: basically okay great all right really appreciate me and you sorry for uh asking some off-topic
[38:12-38:17] Rafael Alonso: questions and thank you for taking extra time definitely not off topic i think it works in your favor
[38:17-38:27] Rafael Alonso: that uh your your questions were product-led uh it's hard to find engineers uh that uh that can
[38:27-38:35] Rafael Alonso: abstract uh away from the code where you know the code is having some impact and that impact is in
[38:35-38:43] Rafael Alonso: the end what matters yeah um yeah so uh i i think this this was uh this was wonderful all right
[38:43-38:49] Brian Takita: actually have more questions um actually um about the installation if you have if you have a moment
[38:50-38:57] Brian Takita: okay thank you i'm sorry about that um so during the installation um is there like
[38:57-39:03] Brian Takita: do you do you have issues with the during installation and do you have somebody live to
[39:04-39:13] Rafael Alonso: sort of mitigate anything that's going on or so we have shifted so it depends the the way that we we
[39:13-39:20] Rafael Alonso: do we installs we have multiple ways so uh because we have different pos systems so we have one pos
[39:20-39:26] Rafael Alonso: system that is geared toward full service these are bigger restaurants higher revenue they require more
[39:26-39:34] Rafael Alonso: white glove and there we have contractors that are usually locally local to the uh the restaurant
[39:34-39:41] Rafael Alonso: where we ship them the devices and then responsible for installing on site okay for lower gpv uh customers
[39:41-39:50] Rafael Alonso: we uh we do remote installs where we're on a zoom with them these are usually simple simpler installs one
[39:50-39:58] Rafael Alonso: station uh maybe a printer maybe a couple of handheld handhelds that's an easier install it can be done
[39:58-40:07] Rafael Alonso: remote remotely and then we have the pure dummy pos which is essentially for retail uh where you don't
[40:07-40:14] Rafael Alonso: even have a catalog you don't have a menu it's just keyed in values um input number put in your card
[40:14-40:20] Rafael Alonso: where it's unboxing experience like you unbox it you plug it in and it's it's ready to go to roll
[40:21-40:26] Brian Takita: okay cool all right well sorry i don't want to keep you anymore but um yeah let's i'll probably have
[40:26-40:34] Rafael Alonso: more questions in the next call so thank you very much yeah no worries uh thank you for for enduring and
[40:34-40:39] Rafael Alonso: we'll we'll get the the next one scheduled as soon as possible okay all right hey i appreciate the call and
[40:39-40:48] Brian Takita: um enjoy your rest of your your evening or is it you too all right see ya guys it is all right bye brian
