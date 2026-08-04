# Local release artifact dry run

This directory is intentionally kept free of binaries and credentials. Build
with the committed lockfile and a maintainer-selected stable toolchain, then
use the `package-artifact` binary described in
`docs/release-checklist.md`. `SOURCE_DATE_EPOCH` must be supplied by the
release owner; it is never read from the environment by the packager.

The packager rejects existing output paths, uses fixed entry order/modes/time,
and writes a SHA-256 manifest outside the archive. Run it twice from identical
source, lockfile, toolchain, target directory inputs, and `SOURCE_DATE_EPOCH`,
then compare archive and manifest bytes. Missing support evidence keeps the
target Experimental even when the bytes reproduce.
