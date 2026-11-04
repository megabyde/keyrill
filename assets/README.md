# Frozen language model

`model.bin` re-encodes the frequency and character-chain data in
[`model/data/model.inc.gz`](../model/README.md) as little-endian alphabet indices and signed
integer micro-units. It preserves all 396,107 six-decimal values exactly; it does not retrain,
recalibrate or renormalize them. `model-provenance.json` binds the frozen source, packer, output
bytes and language counts. `tools/pack-model.py --check` regenerates all assets offline and
compares their bytes.

The model data is derived from wordfreq 3.1.1 under CC-BY-SA-4.0. Retain `model-notice.txt` and
its source attributions with derived data and binaries. `wordfreq-code-license.txt` preserves
the separate upstream code notice; it does not change the data license. This format translation
is a Keyrill modification. Application source is MIT-licensed; that does not relicense the
model data or its upstream inputs.
