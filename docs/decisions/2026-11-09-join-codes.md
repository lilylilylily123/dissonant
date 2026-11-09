---
title: Six-character join codes through a code-keyed lobby, with the host saying yes to each person
status: accepted
date: 2026-11-09
supersedes-part-of: 2026-11-06-live-collaboration
---

# Decision: join with a short code; nobody gets in without the host knowing

## Context

The first invite was the host's whole iroh address plus a 128-bit secret, base64'd: a long line of
text that had to be pasted, and which let in anyone holding it, any number of times, for the whole
session. Nobody was asked. And a guest who knew another guest's client id (which every guest sees)
could reconnect as them without the secret and take their place.

The fix had to stay local-first: no server of ours, no accounts, devices holding their own keys, and
working on one network with no internet as well as across it.

## Options considered

| Option | Verdict |
|---|---|
| **The code stretches into the key of a short-lived lobby endpoint; the host approves each person** | **Chosen.** No infrastructure of ours. Found through n0's address lookup (already used) or mDNS on the local network. |
| Our own rendezvous server mapping codes to addresses | Codes couldn't be hijacked and lookups could be rate limited, but it is a server to run, pay for and keep up, and multiplayer stops when it does. Against the 2026-11-06 decision. |
| Mainline DHT for the code lookup | No n0 dependency at all, but lookups take seconds to minutes. Can be added beside n0 lookup later (`iroh-mainline-endpoint-discovery`). |
| Keep the long invite, add approval only | Doesn't answer "much easier than a very long key". |

## How it works

1. **The code.** Six characters from `23456789ABCDEFGHJKMNPQRSTUVWXYZ` (no 0/O/1/I/L), about 30 bits,
   typed in any case with spaces or dashes. Shown as `K7Q-4MX`.
2. **Stretching.** Argon2id (64 MiB, 3 passes) over the code, salted with the UTC day, gives 64 bytes:
   the lobby's secret key and a separate 32-byte proof. Guessing costs a fraction of a second per try
   per core, and the day in the salt means a table of every code is only good for a day. A guest tries
   the neighbouring day too within an hour of midnight.
3. **The lobby.** The host runs a second iroh endpoint under that key (`dissonant/lobby/1`), findable by
   n0's DNS and, on a local network, by mDNS (service `dissonant`). A guest dials it by key, sends the
   proof, and gets the host's real address back. The lobby takes **one** try, right or wrong: after it
   the code is spent and a new one is made. A wrong proof can only come from something that found the
   lobby without the code (mDNS shows lobby keys to the whole network), so the host is told.
4. **The gate.** The host's real endpoint (`dissonant/session/2`) drops any connection from a device key
   that hasn't come through the lobby in the last ten minutes or been let in, before reading anything.
5. **Approval.** A new person's hello puts them on the host's list and gets them `Waiting`; they see
   nothing of the project and none of the cursors until the host says yes. The host's window asks for
   attention. Saying no, or removing someone, takes their device off the gate.
6. **Words.** Both ends show three words derived (Argon2 again, so they can't be ground out) from the
   host's and the guest's device keys. Anyone in the middle, say someone else who had the code and
   answered as the lobby, would show different words on each side.
7. **Identity.** A guest's place in the session belongs to its device key, which the QUIC handshake
   proves. The client id it sends only names a place; claiming someone else's is refused.
8. **Rotation.** Every 5 minutes, after each use, on "new code", and on a bad knock. Closing the session
   to new people closes the lobby; people already in come back through the gate without asking.

## Consequences

- `PROTOCOL_VERSION` is 2: `Hello` lost its secret and `ToGuest::Waiting` is new.
- Someone holding a live code can still pose as the host to whoever uses that code. The words are what
  catch it, so the UI asks both people to compare them.
- n0's DNS is still a dependency for codes across the internet. On one network, mDNS needs nothing.
- A code takes a few seconds to become findable over the internet; a guest keeps looking for 30
  seconds, so a wrong code takes that long to report.
