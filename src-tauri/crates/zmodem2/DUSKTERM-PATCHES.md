# DuskTerm zmodem2 patches

This directory vendors `zmodem2` 0.7.2 under its original MIT OR Apache-2.0
license. DuskTerm currently carries two isolated protocol additions:

- advertise the local file modification time in the standard ZFILE metadata;
- expose a receiver ZSKIP response as `Event::FileSkipped` instead of reporting
  it as a completed transfer.

The public additions are intentionally limited to `FileInfo` and `Event`, so
the local crate can be replaced when equivalent support is available upstream.
