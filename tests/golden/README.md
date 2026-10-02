Protocol and FEC golden vectors are checked in here and included by crate tests.

`repair-request-v1.txt` uses the manifest-v0 fields. Its tag and CRC were derived separately
with Python hashlib/struct and explicit reflected Castagnoli arithmetic, not the Rust formatter.
