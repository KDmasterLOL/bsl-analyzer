#!/usr/bin/env python3
"""Extract the exact platform global-name surface from 1C:EDT resources.

The input files are taken from the versioned EDT platform plug-in, for example:

  resources/v8.3.27/GlobalContext.type
  resources/v8.3.27/SystemEnums.type

Only names, kinds, availability and global-property writability are copied. The
raw EDT resources are not committed; their SHA-256 hashes make the generated
manifest auditable.
"""

from __future__ import annotations

import argparse
import hashlib
import json
from pathlib import Path
import xml.etree.ElementTree as ET

ENVIRONMENT_BITS = {
    "thick_client": 1,
    "thin_client": 2,
    "web_client": 4,
    "server": 8,
    "mobile_client": 16,
    "external_connection": 32,
}


def environment_mask(raw: str | None) -> int:
    values = set((raw or "").strip("[]").split(",")) - {""}
    mask = 0
    if values & {"MNG_CLIENT", "CLIENT"}:
        mask |= ENVIRONMENT_BITS["thick_client"]
    if values & {"THIN_CLIENT", "MOBILE_THIN_CLIENT"}:
        mask |= ENVIRONMENT_BITS["thin_client"]
    if "WEB_CLIENT" in values:
        mask |= ENVIRONMENT_BITS["web_client"]
    if values & {"SERVER", "MOBILE_SERVER", "MOBILE_AUTONOMOUS_SERVER"}:
        mask |= ENVIRONMENT_BITS["server"]
    if values & {"MOBILE_CLIENT", "MOBILE_THIN_CLIENT"}:
        mask |= ENVIRONMENT_BITS["mobile_client"]
    if "EXTERNAL_CONN" in values:
        mask |= ENVIRONMENT_BITS["external_connection"]
    return mask


def extract(path: Path, xml_tag: str, kind: str) -> dict[tuple[str, str, str], dict[str, object]]:
    symbols: dict[tuple[str, str, str], dict[str, object]] = {}
    for element in ET.parse(path).getroot():
        if element.tag.rsplit("}", 1)[-1] != xml_tag:
            continue
        ru = element.get("nameRu", "")
        en = element.get("name", "")
        # EDT 2026.1.2 contains one anonymous legacy method represented as
        # `?`/`?`. It is not a resolvable source spelling and is excluded.
        if (ru, en) == ("?", "?") or not (ru or en):
            continue
        key = (kind, ru.casefold(), en.casefold())
        symbol = symbols.setdefault(
            key,
            {"kind": kind, "ru": ru, "en": en, "environment_mask": 0},
        )
        symbol["environment_mask"] = int(symbol["environment_mask"]) | environment_mask(
            element.get("environments")
        )
        if kind == "property" and element.get("writable") == "true":
            symbol["writable"] = True
    return symbols


def sha256(path: Path) -> str:
    return hashlib.sha256(path.read_bytes()).hexdigest()


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--global-context", required=True, type=Path)
    parser.add_argument("--system-enums", required=True, type=Path)
    parser.add_argument("--platform-version", required=True)
    parser.add_argument("--edt-version", required=True)
    parser.add_argument("--extracted-at", required=True)
    parser.add_argument("--expected-functions", required=True, type=int)
    parser.add_argument("--expected-properties", required=True, type=int)
    parser.add_argument("--expected-system-enums", required=True, type=int)
    parser.add_argument("--output", required=True, type=Path)
    args = parser.parse_args()

    symbols: dict[tuple[str, str, str], dict[str, object]] = {}
    symbols.update(extract(args.global_context, "methods", "function"))
    symbols.update(extract(args.global_context, "properties", "property"))
    symbols.update(extract(args.system_enums, "properties", "system_enum"))
    kind_order = {"function": 0, "property": 1, "system_enum": 2}
    actual_counts = {
        kind: sum(symbol["kind"] == kind for symbol in symbols.values())
        for kind in kind_order
    }
    expected_counts = {
        "function": args.expected_functions,
        "property": args.expected_properties,
        "system_enum": args.expected_system_enums,
    }
    if actual_counts != expected_counts:
        raise SystemExit(
            "EDT resource counts do not match the independently recorded attestation: "
            f"expected {expected_counts}, got {actual_counts}"
        )
    ordered = sorted(
        symbols.values(),
        key=lambda item: (
            kind_order[str(item["kind"])],
            str(item["en"]).casefold(),
            str(item["ru"]).casefold(),
        ),
    )

    document = {
        "schema_version": 1,
        "source": {
            "provider": "1C:EDT platform model",
            "edt_version": args.edt_version,
            "platform_version": args.platform_version,
            "environment_mask_bits": ENVIRONMENT_BITS,
            "resources": {
                "global_context": {
                    "path": f"resources/v{args.platform_version}/GlobalContext.type",
                    "sha256": sha256(args.global_context),
                },
                "system_enums": {
                    "path": f"resources/v{args.platform_version}/SystemEnums.type",
                    "sha256": sha256(args.system_enums),
                },
            },
            "extracted_at": args.extracted_at,
        },
        "completeness": {"global_context": True, "system_enums": True},
        "attestation": {"expected_symbol_counts": expected_counts},
        "symbols": ordered,
    }
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(json.dumps(document, ensure_ascii=False, indent=2) + "\n")


if __name__ == "__main__":
    main()
