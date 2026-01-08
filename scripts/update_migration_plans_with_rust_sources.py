#!/usr/bin/env python3
"""Update migration plan files with Rust source references"""

import os
import re

RUST_DIR = "~/src/lsp/bsl-language-server-rust/crates/bsl-diagnostics/src/rules"
JAVA_DIR = "~/src/lsp/bsl-language-server/src/main/java/com/github/_1c_syntax/bsl/languageserver/diagnostics"
TARGET_DIR = "~/src/lsp/bsl-analyzer/crates/ide-diagnostics/src/handlers"

# Get all Rust diagnostic files
rust_files = {}
for f in os.listdir(RUST_DIR):
    if f.endswith('.rs') and f not in ['mod.rs', 'test_helpers.rs']:
        name = f[:-3]  # Remove .rs
        rust_files[name] = f

def to_snake_case(name):
    """Convert PascalCase to snake_case"""
    s1 = re.sub('(.)([A-Z][a-z]+)', r'\1_\2', name)
    return re.sub('([a-z0-9])([A-Z])', r'\1_\2', s1).lower()

def pascal_to_diagnostic_name(name):
    """Convert PascalCase to DiagnosticName.java"""
    return f"{name}Diagnostic.java"

def add_sources_section(diagnostic_name, content):
    """Add Sources section after Scope line"""
    snake = to_snake_case(diagnostic_name)
    java_file = pascal_to_diagnostic_name(diagnostic_name)

    # Check if Rust file exists
    has_rust = snake in rust_files

    # Build Sources section
    sources = f'\n**Sources:**\n'
    sources += f'- **Java:** `{JAVA_DIR}/{java_file}`\n'

    if has_rust:
        sources += f'- **Rust Reference:** ✅ `{RUST_DIR}/{snake}.rs`\n'
    else:
        sources += f'- **Rust Reference:** ❌ Not implemented in bsl-language-server-rust\n'

    sources += f'- **Target:** `{TARGET_DIR}/{snake}.rs`\n'

    # Find where to insert (after **Scope:** line)
    scope_pattern = r'(\*\*Scope:\*\* .*?)\n\n'

    if re.search(scope_pattern, content):
        # Insert Sources section after Scope
        new_content = re.sub(
            scope_pattern,
            r'\1\n' + sources + '\n',
            content,
            count=1
        )

        # Add note to Implementation Notes if Rust exists
        if has_rust and '**Implementation Notes:**' in new_content:
            impl_notes_pattern = r'(\*\*Implementation Notes:\*\*\n)'
            new_content = re.sub(
                impl_notes_pattern,
                r'\1- ✅ **Rust reference exists** - can study implementation approach\n',
                new_content,
                count=1
            )

        return new_content

    return content

def process_file(filepath):
    """Process a single migration plan file"""
    print(f"Processing {filepath}...")

    with open(filepath, 'r', encoding='utf-8') as f:
        content = f.read()

    # Find all diagnostic sections (### N. DiagnosticName)
    pattern = r'### \d+\. ([A-Z][a-zA-Z0-9]+)\n\n\*\*Code:\*\*'

    for match in re.finditer(pattern, content):
        diagnostic_name = match.group(1)
        print(f"  Found: {diagnostic_name}")

        # Find the full diagnostic section (from ### to next ### or end)
        start = match.start()
        next_section = re.search(r'\n---\n\n### \d+\.', content[start+10:])

        if next_section:
            end = start + 10 + next_section.start()
            section = content[start:end]
        else:
            # Last diagnostic in file
            section = content[start:]

        # Check if Sources section already exists
        if '**Sources:**' in section:
            print(f"    Skipped (Sources already exists)")
            continue

        # Add Sources section
        new_section = add_sources_section(diagnostic_name, section)

        if new_section != section:
            content = content.replace(section, new_section)
            print(f"    ✅ Added Sources section")

    # Write back
    with open(filepath, 'w', encoding='utf-8') as f:
        f.write(content)

    print(f"✅ Done: {filepath}\n")

def main():
    plan_dir = "~/src/lsp/bsl-analyzer/docs/planning/diagnostics"

    files = [
        f"{plan_dir}/MIGRATION_PLAN_A_C.md",
        f"{plan_dir}/MIGRATION_PLAN_D_M.md",
        f"{plan_dir}/MIGRATION_PLAN_N_S.md",
        f"{plan_dir}/MIGRATION_PLAN_T_Z.md"
    ]

    for filepath in files:
        if os.path.exists(filepath):
            process_file(filepath)
        else:
            print(f"❌ File not found: {filepath}")

    print("=" * 60)
    print(f"Total Rust diagnostics available: {len(rust_files)}")
    print("All migration plans updated!")

if __name__ == "__main__":
    main()
