# What This Does, In Plain Terms

You have heard the word "stingray," or "IMSI catcher," or "cell-site
simulator." Maybe from a news story, maybe from someone who worries about
these things. You want to know what they are and whether this tool helps,
without a lecture on how phones work. This page is that, in about five minutes,
with no jargon. If you want the deeper version afterward, every idea here has a
fuller page later in the book.

## The problem, in one picture

Your phone is useful because it is always in touch with the phone network. To
stay in touch, it constantly looks for the nearest, strongest connection point
and links up with it — automatically, without asking you, dozens of times a
day as you move around. This is normal and it is how every phone has always
worked.

The catch is that your phone is trusting. When it sees something that
announces itself as a legitimate connection point, it tends to believe it and
connect, before it has really checked. Most of the time that is fine, because
the thing really is your phone company's equipment.

But someone can build a device that pretends to be that equipment. Your phone,
being trusting, connects to it. And now that device can do things: learn a
permanent code that identifies you, work out where you are, or in some cases
listen to what you send. These fake connection points are what people mean by
"stingray" or "IMSI catcher." They are real, they are used by police and others
around the world, and to your phone they are invisible — no warning, no sign,
nothing on your screen.

## What Rayhunter does about it

Rayhunter is a small device — a cheap mobile hotspot with special software on
it — that watches the connection your phone network makes and looks for the
tell-tale signs of one of these fakes at work.

Think of it as a smoke detector for your phone signal. It sits quietly. It does
not make calls or send anything out; it only listens. When it notices a pattern
that matches how these fake devices behave, it raises a warning, so that
something normally invisible becomes something you can at least see.

That comparison also carries the tool's honest limits, so it is worth taking
seriously:

- **A smoke detector tells you there might be a fire. It does not put it out.**
  Rayhunter can tell you a warning sign appeared. It cannot stop anything, and
  it cannot protect your phone.
- **A smoke detector sometimes goes off when you burn toast.** Not every
  warning means a real attack. Ordinary, harmless quirks of the phone network
  can set it off too, and learning to tell the difference is a real part of
  using it.
- **A quiet detector is not a promise of safety.** No alarm means it did not
  see anything it recognises — not that nothing could possibly be happening.

None of that makes it useless. A smoke detector is worth having. But knowing
exactly what it is for keeps you from trusting it too much or too little, and
this whole book is built to help you get that balance right.

## What it does not do

To be clear about the boundaries from the start:

- It does **not** hide you, protect your phone, or block anything. It watches;
  it does not defend.
- It does **not** tell you who is behind a warning, or why. It sees the
  behaviour, not the hand behind it.
- It does **not** prove you were targeted. A warning is a reason to look
  closer, not a verdict.
- It **cannot** promise it will catch everything. Some things are beyond what
  any tool like this can see.

These are not fine print. They are the difference between using Rayhunter well
and being misled by it, which is why the book keeps returning to them.

## Is it for you?

Maybe. For some people — those with specific, real reasons to worry about this
kind of surveillance — it is a genuinely useful instrument. For many others it
is fascinating but not actually protective, and it is worth being honest with
yourself about which you are before you invest time in it. The next page,
[Is This For Me?](./threat-models.md), walks through who tends to benefit and
who does not, without trying to talk you into anything.

## Where to next

- To figure out whether Rayhunter fits your situation, read
  [Is This For Me?](./threat-models.md).
- If you already have a device and want to get it running, jump to the
  [Quick Start](./quick-start.md).
- If you want to understand the real machinery behind all of this — what these
  fake devices actually do, and how the detection works — the
  [Understanding the problem](./concepts/cell-networks.md) section builds it up
  from the beginning, still in plain language.
