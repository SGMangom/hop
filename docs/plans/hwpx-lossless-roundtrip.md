# HWPX lossless package round-trip

## Problem

An actual Hancom Office 2024 HWPX sample contains `Scripts/headerScripts.js`
and `Scripts/sourceScripts.js`. HOP/rhwp parsed the document successfully but
dropped both package parts when the document was saved again. That is silent
data loss even though HOP does not execute document scripts.

## Goal

Preserve script package parts inertly and byte-for-byte when an HWPX document is
opened and saved as HWPX. Keep the OPF manifest/spine synchronized with the ZIP
contents and retain the existing legacy extensionless script lookup as a
fallback for older corpus files.

## Safety / non-goals

- Scripts are data only; HOP must not execute them.
- Do not add macro/VBA/ActiveX execution.
- Do not weaken encrypted-document handling or ZIP validation.
- Do not edit the pinned `third_party/rhwp` checkout; apply a deterministic HOP
  patch to the generated engine copy.

## Acceptance gates

1. Parser retains both standard `.js` script parts as HWPX auxiliary entries.
2. Serializer writes the retained bytes unchanged.
3. `Contents/content.hpf` declares every retained script and spine entry.
4. HWPX→HWP contract extraction recognizes standard `.js` paths and retains the
   existing extensionless fallback.
5. Real Hancom 2024 sample: no ZIP package entry is lost, both script payloads
   are byte-identical, page count remains 5 after export/reparse.
