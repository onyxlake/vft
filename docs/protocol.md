# Protocol v0

All integers are unsigned little endian. Parsing rejects unknown versions, flags, packet types,
profiles, FEC algorithms and compression modes. There is no required ACK or network channel.

## Packet

| Offset | Bytes | Field |
| --- | --- | --- |
| 0 | 4 | ASCII `VFT0` |
| 4 | 1 | Version = 0 |
| 5 | 1 | Type: manifest 0, source 1, repair 2, control 3 |
| 6 | 1 | Profile: robust 0, balanced 1 |
| 7 | 1 | Reserved = 0 |
| 8 | 16 | Session ID |
| 24 | 16 | Object ID |
| 40 | 4 | Generation ID |
| 44 | 4 | Symbol index within generation |
| 48 | 4 | Payload length, at most 65,535 bytes |
| 52 | variable | Payload |
| 52 + length | 4 | CRC32C of header and payload |

CRC32C uses reflected Castagnoli polynomial `0x82f63b78`, initial state `0xffffffff`, final XOR
`0xffffffff`. Check value for ASCII `123456789` is `0xe3069283`. Exact packet length is mandatory.
Manifest packets use generation/index zero. Control packets are reserved and ignored in v0.

## Manifest payload

| Offset | Bytes | Field |
| --- | --- | --- |
| 0 | 1 | Version = 0 |
| 1 | 1 | FEC type = 0 (binary fountain v0) |
| 2 | 1 | Visual profile |
| 3 | 1 | Compression = 0 (none) |
| 4 | 16 | Session ID, must match packet |
| 20 | 16 | Object ID, must match packet |
| 36 | 8 | Original file length |
| 44 | 32 | SHA-256 of original file bytes |
| 76 | 4 | Source symbol length |
| 80 | 4 | Source symbol count |
| 84 | 2 | UTF-8 filename length |
| 86 | variable | UTF-8 filename, 1..255 bytes |

Symbol count must equal `ceil(file_length / symbol_length)`; an empty object has zero symbols.
Filenames cannot contain NUL, slash, backslash or colon, or be `.`/`..`. Receivers use the explicitly
selected output path, not the transmitted name. Symbols are zero-padded to their declared length;
padding is excluded from the final SHA-256. Maximum symbol count is `u32::MAX`.

Generation size is fixed at 64 source symbols. Generation count is `ceil(source_count / 64)`.
The last generation can be shorter. Source index `i < k` denotes a unit coefficient vector;
repair indices start at `k`, where `k` is that generation's source count.

## Deterministic FEC

The generation seed is the first eight bytes, interpreted little endian, of
`SHA256(encoded_manifest || generation_id_le32)`.

For repair index `i`, compute `SplitMix64(seed XOR u64(i))`, mask to the low `k` bits. For `k=64`
the mask is all ones. If the coefficient mask is zero, use `1 << (i % k)`. A repair payload is
the XOR of all source symbols selected by this mask. All arithmetic below wraps modulo 2^64:

```text
v += 0x9e3779b97f4a7c15
v = (v XOR (v >> 30)) * 0xbf58476d1ce4e5b9
v = (v XOR (v >> 27)) * 0x94d049bb133111eb
return v XOR (v >> 31)
```

Decoding uses Gaussian elimination over GF(2). Recovery needs rank `k`, not merely `k` arrivals.
Dependent repairs are expected; the sender continuously generates additional deterministic indices.
This is a small dense binary fountain code, not RaptorQ, and offers no fixed loss-recovery guarantee
for a given number of packets. A future FEC algorithm must use a new type ID and new golden vectors.

## Scheduling and reception

Frame zero is a manifest. By default every 30th frame repeats it. All source symbols are sent once,
then 64 repairs per generation are sent in round-robin order. New rounds use new symbol IDs.
An index overflow returns an error instead of wrapping into a previous ID.

A receiver waits for a valid manifest, pins the first session/object, ignores other sessions, and
rejects conflicting metadata for the same identity. It does not buffer unknown symbols before
discovery. This permits joining midstream through repeated manifests and repair-only recovery.

CRC protects each packet. COMPLETE requires reconstruction of every generation and a matching
SHA-256. A hash mismatch is a terminal error; no output file is created from that object. Neither
checksum authenticates the sender.

The recent duplicate cache defaults to 4,096 symbol IDs. Older duplicates remain harmless because
FEC rejects dependent equations and completed generations ignore new symbols. Duplicate statistics
count cache hits; valid symbols include dependent equations, and ignored symbols include arrivals
for completed generations. Receiver defaults limit file length to 256 MiB and source count to
1,000,000. Limits are configurable; callers must budget memory before raising them.

Golden vectors in `tests/golden/` fix packet, manifest and repair bytes. They were independently
generated using Python `struct`, `hashlib`, and explicit CRC/FEC arithmetic.

## Optional manual repair text (VFR1)

`vft-protocol::repair_request` parses human input independently of visual packets. Group numbers
are one-based in text and zero-based in the core. Simple lists (`18,42,90-91`) request sixteen
fresh repairs per group; `group:deficit` uses `max(8, deficit + ceil(deficit/2) + 4)` packets.
Deficits must be 1..k, including the last short generation. Duplicate/overlapping groups are errors.

Full codes are `VFR1/TAG/REQUEST-ID/group:deficit,.../CRC`. TAG is the first eight SHA-256 bytes
of the complete canonical manifest, as sixteen uppercase hexadecimal digits. REQUEST-ID is an
adapter-generated random u64 in sixteen hexadecimal digits. Entries use ascending groups and
canonical decimal numbers. CRC is CRC32C of the canonical ASCII prefix through entries, as eight
hexadecimal digits. Outer whitespace and hexadecimal case are accepted; other syntax is strict.
Full codes never fall back to simple parsing. Limits are 4096 input bytes, 256 expanded targets,
128 repairs per group and 32768 repairs per request. Tags/checksums detect mistakes, not identity.

The independently derived manifest-v0 vector produces tag `89BF47CBF57E2AAB` and request
`VFR1/89BF47CBF57E2AAB/0123456789ABCDEF/1:1/BC78F06C`. The existing binary wire format is unchanged.

Manual scheduling uses one active budget and a common monotonic repair-index allocator per group.
Automatic position stays independent: cancellation drops only unsent budget, and sources resume
at their original cursor. Manifests keep priority at the global interval. Manual groups rotate
after at most eight packets; each 32 manual data packets yields one ordinary data packet.
The recent 64 full request IDs make equal repeated requests no-ops and reject conflicting content.
Manual cursor metadata is allocated lazily, capped at 8 MiB including request history. Refusal
leaves automatic transmission available. Read/encode failure preserves scheduling and budgets;
generation-load/read-byte counters expose the cost of retaining only one generation cache.
