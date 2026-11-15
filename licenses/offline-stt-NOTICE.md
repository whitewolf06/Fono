# Offline STT model attribution

GigaAM v3 is a model by the GigaChat Team. The selected `v3_e2e_rnnt` model was
converted to sherpa-compatible ONNX by Fangjun Kuang / Xiaomi Corp. and uses
an INT8 encoder. The conversion does not change its declared MIT license.
The exact package revision is `a6039be7cee829a9044a69ac0ebaf1c191217c97` in
[the publisher's repository](https://huggingface.co/csukuangfj/sherpa-onnx-nemo-transducer-punct-giga-am-v3-russian-2025-12-16).
Its included `LICENSE` says `Copyright (c) 2024 GigaChat Team`;
the manifest pins that complete notice along with the model files.

Parakeet TDT 0.6B v3 is a model by NVIDIA. The selected ONNX INT8 conversion is
published by Fangjun Kuang at revision
`2bda32ec70b097a55adaa07d9a7173915b43cc78` in
[the conversion repository](https://huggingface.co/csukuangfj/sherpa-onnx-nemo-parakeet-tdt-0.6b-v3-int8).
ONNX export and INT8 quantization are changes from the original model format.
The [original model card](https://huggingface.co/nvidia/parakeet-tdt-0.6b-v3)
declares [CC-BY-4.0](https://creativecommons.org/licenses/by/4.0/legalcode.en).
Retain model attribution, these source/license links and the conversion notice
when distributing or activating a downloaded package. No endorsement by NVIDIA
or the model/conversion authors is implied.

These model terms are independent of Fono's MIT license. The selected weights
are separate downloads and are not included in the Fono source repository.
The shared runtime's existing notices remain in
[THIRD_PARTY_NOTICES.md](../../../THIRD_PARTY_NOTICES.md).
