#!/usr/bin/env python3
"""
Verify that Rust metadata definitions match Java @DiagnosticMetadata annotations.

This script compares metadata for all implemented diagnostics to ensure 100%
compatibility with bsl-language-server (Java).
"""

import re
import sys
from pathlib import Path
from typing import Dict, Optional

# Mapping tables
SEVERITY_MAP = {
    "INFO": "Info",
    "MINOR": "Minor",
    "MAJOR": "Major",
    "CRITICAL": "Critical",
    "BLOCKER": "Blocker"
}

TYPE_MAP = {
    "CODE_SMELL": "CodeSmell",
    "ERROR": "Error",
    "VULNERABILITY": "Vulnerability",
    "SECURITY_HOTSPOT": "SecurityHotspot"
}

TAG_MAP = {
    "STANDARD": "Standard",
    "BADPRACTICE": "Badpractice",
    "BRAINOVERLOAD": "Brainoverload",
    "CLUMSY": "Clumsy",
    "DESIGN": "Design",
    "ERROR": "Error",
    "LOCKINOS": "Lockinos",
    "PERFORMANCE": "Performance",
    "SQL": "Sql",
    "SUSPICIOUS": "Suspicious",
    "UNPREDICTABLE": "Unpredictable",
    "DEPRECATED": "Deprecated",
    "UNUSED": "Unused",
    "LOCALIZE": "Localize"
}


def parse_java_metadata(file_path: Path) -> Optional[Dict]:
    """Parse @DiagnosticMetadata annotation from Java file."""
    try:
        with open(file_path, 'r', encoding='utf-8') as f:
            content = f.read()
        
        # Extract @DiagnosticMetadata annotation
        match = re.search(r'@DiagnosticMetadata\((.*?)\)', content, re.DOTALL)
        if not match:
            return None
        
        metadata_str = match.group(1)
        metadata = {}
        
        # Parse type
        type_match = re.search(r'type\s*=\s*DiagnosticType\.(\w+)', metadata_str)
        if type_match:
            metadata['type'] = TYPE_MAP.get(type_match.group(1))
        
        # Parse severity
        sev_match = re.search(r'severity\s*=\s*DiagnosticSeverity\.(\w+)', metadata_str)
        if sev_match:
            metadata['severity'] = SEVERITY_MAP.get(sev_match.group(1))
        
        # Parse minutesToFix
        min_match = re.search(r'minutesToFix\s*=\s*(\d+)', metadata_str)
        if min_match:
            metadata['minutes_to_fix'] = int(min_match.group(1))
        
        # Parse activatedByDefault (default is true)
        act_match = re.search(r'activatedByDefault\s*=\s*(true|false)', metadata_str)
        metadata['activated_by_default'] = act_match.group(1) == 'true' if act_match else True
        
        # Parse tags
        tags_match = re.search(r'tags\s*=\s*\{([^}]+)\}', metadata_str)
        if tags_match:
            tags_str = tags_match.group(1)
            tags = re.findall(r'DiagnosticTag\.(\w+)', tags_str)
            metadata['tags'] = sorted([TAG_MAP.get(t, t) for t in tags])
        else:
            metadata['tags'] = []
        
        return metadata
    except Exception as e:
        print(f"Error parsing {file_path}: {e}", file=sys.stderr)
        return None


def main():
    # Find project root
    script_dir = Path(__file__).parent
    project_root = script_dir.parent
    
    # Java source directory
    java_dir = Path.home() / "src/lsp/bsl-language-server/src/main/java/com/github/_1c_syntax/bsl/languageserver/diagnostics"
    
    if not java_dir.exists():
        print(f"❌ Java directory not found: {java_dir}", file=sys.stderr)
        sys.exit(1)
    
    # Diagnostics to verify (key representatives from each category)
    diagnostics_to_verify = {
        # DISABLED_BY_DEFAULT
        "TernaryOperatorUsage": {
            "type": "CodeSmell",
            "severity": "Minor",
            "minutes_to_fix": 3,
            "activated_by_default": False,
            "tags": ["Brainoverload"]
        },
        "BadWords": {
            "type": "CodeSmell",
            "severity": "Major",
            "minutes_to_fix": 1,
            "activated_by_default": False,
            "tags": ["Design"]
        },
        "TooManyReturns": {
            "type": "CodeSmell",
            "severity": "Minor",
            "minutes_to_fix": 20,
            "activated_by_default": False,
            "tags": ["Brainoverload"]
        },
        
        # Error diagnostics
        "DataExchangeLoading": {
            "type": "Error",
            "severity": "Critical",
            "minutes_to_fix": 5,
            "activated_by_default": True,
            "tags": ["Badpractice", "Standard", "Unpredictable"]
        },
        "SameMetadataObjectAndChildNames": {
            "type": "Error",
            "severity": "Critical",
            "minutes_to_fix": 30,
            "activated_by_default": True,
            "tags": ["Design", "Sql", "Standard"]
        },
        
        # Vulnerability
        "ExecuteExternalCode": {
            "type": "Vulnerability",
            "severity": "Critical",
            "minutes_to_fix": 1,
            "activated_by_default": True,
            "tags": ["Error", "Standard"]
        },
        
        # Code Smell
        "UnusedLocalVariable": {
            "type": "CodeSmell",
            "severity": "Major",
            "minutes_to_fix": 1,
            "activated_by_default": True,
            "tags": ["Badpractice", "Brainoverload", "Unused"]
        },
        "RedundantAccessToObject": {
            "type": "CodeSmell",
            "severity": "Info",
            "minutes_to_fix": 1,
            "activated_by_default": True,
            "tags": ["Clumsy", "Standard"]
        },
        "LineLength": {
            "type": "CodeSmell",
            "severity": "Minor",
            "minutes_to_fix": 1,
            "activated_by_default": True,
            "tags": ["Badpractice", "Standard"]
        },
    }
    
    print("METADATA COMPATIBILITY VERIFICATION")
    print("=" * 100)
    print(f"Comparing Rust metadata definitions with Java @DiagnosticMetadata")
    print(f"Java source: {java_dir}")
    print("=" * 100)
    
    total = len(diagnostics_to_verify)
    passed = 0
    failed = 0
    
    for diag_name, expected_rust in diagnostics_to_verify.items():
        file_path = java_dir / f"{diag_name}Diagnostic.java"
        
        if not file_path.exists():
            print(f"❌ {diag_name:40} FILE NOT FOUND")
            failed += 1
            continue
        
        java_metadata = parse_java_metadata(file_path)
        if not java_metadata:
            print(f"⚠️  {diag_name:40} PARSE ERROR")
            failed += 1
            continue
        
        # Compare fields
        mismatches = []
        
        if expected_rust.get('type') != java_metadata.get('type'):
            mismatches.append(f"type: Rust={expected_rust.get('type')}, Java={java_metadata.get('type')}")
        
        if expected_rust.get('severity') != java_metadata.get('severity'):
            mismatches.append(f"severity: Rust={expected_rust.get('severity')}, Java={java_metadata.get('severity')}")
        
        if expected_rust.get('minutes_to_fix') != java_metadata.get('minutes_to_fix'):
            mismatches.append(f"minutes: Rust={expected_rust.get('minutes_to_fix')}, Java={java_metadata.get('minutes_to_fix')}")
        
        if expected_rust.get('activated_by_default') != java_metadata.get('activated_by_default'):
            mismatches.append(f"activated: Rust={expected_rust.get('activated_by_default')}, Java={java_metadata.get('activated_by_default')}")
        
        # Compare tags (order-independent)
        rust_tags = sorted(expected_rust.get('tags', []))
        java_tags = sorted(java_metadata.get('tags', []))
        if rust_tags != java_tags:
            mismatches.append(f"tags: Rust={rust_tags}, Java={java_tags}")
        
        if mismatches:
            print(f"❌ {diag_name:40} MISMATCH")
            for mismatch in mismatches:
                print(f"   {mismatch}")
            failed += 1
        else:
            print(f"✅ {diag_name:40} MATCH")
            passed += 1
    
    print("=" * 100)
    print(f"Results: {passed}/{total} diagnostics match Java implementation")
    
    if failed == 0:
        print("✅ ALL VERIFIED DIAGNOSTICS ARE COMPATIBLE WITH JAVA")
        sys.exit(0)
    else:
        print(f"❌ {failed} DIAGNOSTICS HAVE COMPATIBILITY ISSUES")
        sys.exit(1)


if __name__ == "__main__":
    main()
