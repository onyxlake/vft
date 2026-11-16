# VFT Git application package v1

A `.vftgit` file is a 128-byte header followed immediately by one native Git bundle. This is
an application-level object transported as an ordinary VFT file. It does not change the VFT
v0 manifest, packet, profile, FEC, CRC or visual formats. The VFT manifest SHA-256 covers the
entire file, including this header; the inner SHA-256 covers only the bundle bytes.

## Header layout

All integers use little-endian byte order. Offsets and lengths are in bytes.

| Offset | Length | Value |
| --- | --- | --- |
| 0 | 8 | Magic: ASCII `VFTGIT` followed by CR LF (`56 46 54 47 49 54 0d 0a`) |
| 8 | 2 | Version: unsigned 16-bit integer 1 |
| 10 | 2 | Header length: unsigned 16-bit integer 128 |
| 12 | 1 | Git object format: 1 = SHA-1, 2 = SHA-256 |
| 13 | 3 | Reserved; all zero |
| 16 | 8 | Bundle byte length, unsigned 64-bit integer, greater than zero |
| 24 | 32 | Baseline commit OID: raw bytes; SHA-1 uses first 20 and zero-pads 12 |
| 56 | 32 | Target commit OID: raw bytes; same padding rule |
| 88 | 32 | SHA-256 digest of the bundle payload |
| 120 | 8 | Reserved; all zero |
| 128 | Bundle length | Native Git bundle bytes |

Both OIDs must be nonzero and distinct. They are full commit identities, not abbreviated hashes,
branch names or revision expressions. SHA-1 OIDs render as 40 lowercase hex digits, SHA-256 as
64. No filesystem paths or receiving branch names are encoded.

Readers reject unknown magic/version/header length/object format, nonzero reserved bytes or
OID padding, zero/equal OIDs, zero length, overflow in `128 + bundle_length`, and any actual
file length different from that sum. Trailing bytes are invalid. They check the total against
the configured package limit before copying the body. The default is 268,435,456 bytes, including
the header. Hash/copy operations use 65,536-byte buffers; no header field controls an unbounded
memory allocation. Native Git can require additional storage for expanded objects.

## Bundle semantics

The sender creates a native version-3 bundle for a temporary reference pinned to T, excluding
history reachable from B. Merge history may cause Git to describe more than one prerequisite.
Prerequisites and pack data use Git's own format and are verified by the receiving Git runtime.
The receiver requires exactly one advertised head with OID T. Its advertised reference name is
never used as a destination ref. Objects are imported with `bundle unbundle`; application code
subsequently verifies that T is a commit, the object database is complete, B is an ancestor of T,
and the selected receiving branch is still at its startup commit (B or T for idempotent retry).
A bundle's successful object import alone cannot advance the receiving branch or report success.

The actual destination branch and repository are local CLI choices. Updating them follows the
[Git synchronization checks and cancellation boundaries](git-sync.md#validation-application-and-retry).
No reverse-channel negotiation or acknowledgement is added to VFT.

## Independent header fixture

`apps/vft-git/tests/vectors/header-v1.hex` is a 128-byte header fixture assembled independently
with Python `struct.pack('<HHBBHQ', 1, 128, 1, 0, 0, 3)`, explicit raw OID bytes and Python's
SHA-256 of ASCII `abc`. Its baseline is bytes `00` through `13`, target `14` through `27`,
followed by each OID's 12-byte zero padding. The digest is:

```text
ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad
```

The fixture validates the application header independently of the Rust encoder. Its three-byte
payload is intentionally not a valid Git bundle. Separate isolated-repository tests generate
real SHA-1/SHA-256 bundles and exercise Git prerequisites, malformed packages and application.
