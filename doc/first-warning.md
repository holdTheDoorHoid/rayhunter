# Your First Warning

The moment Rayhunter raises its first warning is the moment that matters most,
and it is the worst time to be learning where everything is. So this tutorial
has you produce a warning **on purpose** (a clearly-labelled fake one, using
Rayhunter's built-in demo) and walk through reading it, start to finish, while
nothing is actually wrong. When a real one arrives, you will have done this
before.

By the end you will have made a practice warning appear, found it, seen which
detector fired and how serious it is, looked at the actual messages behind it,
and turned the demo back off. It takes about ten minutes.

You need Rayhunter installed and its dashboard open. If you are not there yet,
do the [Quick Start](./quick-start.md) first.

## Step 1: Turn on the demo button

The demo is off by default, because it writes fake data into a recording and
nobody should do that by accident. Turn it on:

1. In the dashboard, open the settings (the configuration page).
2. Find the **Demonstration** section.
3. Check **"Enable the demo warning button."**
4. Save.

**Expected result:** after you save, Rayhunter restarts itself, which takes
about a minute. The dashboard may be unresponsive during that time, that is
normal, not a failure. When it comes back, a small amber, dashed-outline panel
labelled **"Demonstration mode"** is on the page.

*If no amber panel appears after the restart:* reload the page. If it is still
missing, re-open settings and confirm the demo checkbox is still ticked and was
saved.

## Step 2: Simulate a detection

In that amber "Demonstration mode" panel, click **"Simulate a detection."**

**Expected result:** a message saying the demo warning was injected and will
appear in the history within a few seconds. On the device itself, the status
line changes colour, exactly as it would for a real detection.

The panel is deliberately styled to look like a demo, and the warning it makes
is clearly labelled as fake. This is on purpose: anyone glancing at the screen,
or at a screenshot, should be able to tell this device is being demonstrated and
not reporting a real catch.

*If you get an error instead:* the demo mode did not fully turn on. Go back to
Step 1, confirm the checkbox is saved, and wait for the restart to finish before
trying again.

## Step 3: Find the warning

Look at the recording history and the current recording on the dashboard. Within
a few seconds, the recording you are making shows a warning.

**Expected result:** a warning entry appears, with a severity colour and a
count. This is the same place, and the same look, a real warning would have.

*If nothing appears after fifteen seconds:* make sure a recording is actually
running (the Quick Start leaves one running; if you had stopped it, start a new
one and simulate again).

## Step 4: Read what it says

A warning tells you three things before you open anything. Read them in this
order:

1. **Its severity**, Low, Medium, or High, shown by colour and label. This is
   Rayhunter's estimate of how much the finding is worth. [Severity, and What It
   Means](./severity.md) defines the levels.
2. **Which detector fired**, the name of the check that raised it, which tells
   you *what kind* of thing was seen (an identity request, a downgrade, a
   location request, and so on). Each has a page under the [Detector
   Reference](./detectors/index.md).
3. **The message text**, a plain sentence describing what was seen. A demo
   warning's text is marked as demo data.

**Expected result:** you can say, in a sentence, "a [severity] warning from the
[name] detector, about [what the message says]." That sentence is the whole
point of this step.

## Step 5: Open the recording and see the messages

Now go deeper, into the actual signalling behind the warning. Open the recording
and find the [packet explorer](./packet-explorer.md), the view that lists the
individual messages in the recording.

The messages that raised a warning are marked, and you can jump to them. Open the
one behind your demo warning.

**Expected result:** you see the specific message the detector reacted to, in
context with the messages around it. For a real warning, this is what lets you, or someone you share the recording with, check the finding rather than take it
on faith. For this demo one, it is where you learn to look.

*If the packet explorer is empty or still loading:* give the recording a few
seconds to be analysed. A very fresh recording may not have been processed yet.

## Step 6: Decide what it means, and practice not overreacting

Here is the part worth rehearsing most. You now have everything the tool can
give you: a severity, a detector, a message, and the packets. The remaining
question, *what does this mean?*, is not one the tool answers. It is yours.

For this demo warning the answer is plain: it means the demo works. But the habit
to build is the real one. A single warning is a lead, not a verdict; what raises
or lowers your confidence depends on where you are, whether it repeats, and
whether other detectors agree. **[Reading Warnings Without
Panicking](./concepts/interpreting-warnings.md) is the page that teaches this,
and it is the one to read now, while the stakes are zero.** Do not skip it, it
is the difference between using Rayhunter well and being frightened by it.

## Step 7: Turn the demo back off

Practice over. Clean up so your real recordings stay trustworthy:

1. Go back to settings and **un-check "Enable the demo warning button,"** then
   save (another minute-long restart).
2. Start a **fresh recording**, so the one containing demo data is set aside.

**Expected result:** the amber demo panel is gone, and your new recording
contains only real traffic.

**Important:** a recording that contains demo data is not evidence, and should
never be sent to EFF or presented as a real detection. Setting it aside and
starting fresh is how you keep that line clear.

## You are ready

You have now done, on a harmless practice warning, exactly what you would do with
a real one: found it, read its severity and detector, seen the messages behind
it, and, most importantly, treated the question of what it means as a
judgement rather than a verdict. That rehearsal is the point.

Read next, if you have not:

- [Reading Warnings Without Panicking](./concepts/interpreting-warnings.md), how
  to weigh a real warning.
- [The Web Interface, Panel by Panel](./web-interface.md), everything else on
  the dashboard.
- [Sharing What You Find](./sharing-findings.md), how to report a real finding
  without overstating it, and how to redact it first.
