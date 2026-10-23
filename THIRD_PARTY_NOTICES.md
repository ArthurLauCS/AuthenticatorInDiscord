# Third-party sources

`proto/googleauth.proto` is copied unchanged from [JeaNile/gotp](https://github.com/JeaNile/gotp), commit `f49ac7fd5e2445e62f1c6e6e3b449925ba685c2f`, under the MIT license. See `third_party/gotp-LICENSE`.

`authbot/googleauth_pb2.py` is generated from that schema with `grpcio-tools==1.84.1`:

```powershell
python -m grpc_tools.protoc -I proto --python_out=authbot proto/googleauth.proto
```

The bot uses the schema and migration format; it does not invoke gotp or copy its OTP implementation. OTP generation uses PyOTP and preserves the imported hash algorithm and digit count. Only time-based TOTP entries are supported. Unsupported entries or incomplete multi-QR batches fail before writing any entries.
