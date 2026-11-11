# Test fixtures

Unit tests generate deterministic byte fixtures in memory. Native Windows and Linux
test harnesses write generated source/recovered files under ignored `output/`, then
compare independent SHA-256 hashes. Fixture generation is a tooling convention, not
part of the wire format. See [development scripts](../../docs/development.md) for commands
and the current Python fixture algorithm.
