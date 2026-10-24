# Third-party sources

`proto/googleauth.proto` is copied unchanged from [JeaNile/gotp](https://github.com/JeaNile/gotp), commit `f49ac7fd5e2445e62f1c6e6e3b449925ba685c2f`, under the MIT license. See `third_party/gotp-LICENSE`.

It documents the Google Authenticator export format (`otpauth-migration://offline?data=…`). `core/src/lib.rs` decodes that format by hand following the field numbers and enum values in this schema; no code is generated from it and no gotp code is used.
