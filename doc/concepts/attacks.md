# How These Attacks Actually Work

A cell-site simulator is not one trick. It is a position, pretending to be a
tower your phone trusts, from which several different things can be done to
you. This section takes the two operating modes from
[What a Cell-Site Simulator Is](./cell-site-simulators.md) and breaks them
into the four specific techniques Rayhunter watches for.

This page is a map. Each technique gets a paragraph here and a full page of
its own; read the ones that concern you and skip the rest. They are not
alternatives, a single device may use several in sequence, and in fact the
later ones often depend on the earlier ones working first.

## The four families

### Capturing your identity

The network asks your phone for the permanent number that identifies it, at a
moment when it should not need to, and without ever proving it is a real
network. This is the most common technique and the one built for harvesting
everyone in an area at once. It answers the question "who was here?" Full
page: [Capturing Your Identity](./attack-identity.md).

### Downgrading you to a weaker network

Your phone is pushed off a modern network onto an old one, usually 2G, whose defenses are weak or absent. On its own a downgrade may reveal little,
but it is the doorway to the next two techniques, because the protections it
strips away are the ones that would have stopped them. Full page:
[Downgrading You to Weaker Networks](./attack-downgrade.md).

### Turning off encryption

The network tells your phone to communicate with no encryption, or with an
encryption known to be breakable. With the lock removed, whatever passes
between your phone and the tower can be read, and sometimes altered, by
whoever is operating the tower. Full page:
[Turning Off Encryption](./attack-encryption.md).

### Making your phone report its position

The network uses a legitimate feature, the one that lets an emergency call
be located, to ask your phone to measure and report exactly where it is.
This is more precise than any of the others: it does not estimate your
location, it asks your device to state it, and it can ask for that report to
repeat continuously. Full page:
[Making Your Phone Report Its Position](./attack-location.md).

## How they fit together

A useful way to hold these in mind is by what each one gives the operator:

- Identity capture answers **who** you are.
- Location reporting answers **where** you are.
- Downgrade and encryption removal are about **what you are saying**, they
  are the steps that turn a tower you connected to into a tower that can read
  your traffic.

The techniques also build on each other. Downgrading you to 2G, for example,
makes turning off encryption trivial, because 2G's protections were weak to
begin with. This is why seeing two detectors fire close together carries more
weight than either alone, a point [Reading Warnings Without
Panicking](./interpreting-warnings.md) returns to, and one worth remembering
before you read a single warning as the whole story.

Two threads run underneath all four, and each has its own page rather than
being repeated in every technique: [Why Detection Is
Hard](./why-detection-is-hard.md) explains why none of these can be caught
with certainty, and [What This Tool Cannot Tell You](./limitations.md) marks
the edge of what a passive detector can see at all.
