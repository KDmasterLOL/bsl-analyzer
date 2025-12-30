#!/bin/bash
# Extract diagnostic metadata from Java source files

BSL_LS_PATH="/Users/kiriller/src/lsp/bsl-language-server"
DIAG_PATH="$BSL_LS_PATH/src/main/java/com/github/_1c_syntax/bsl/languageserver/diagnostics"

echo "# Diagnostic Metadata Extraction"
echo ""
echo "| Key | Type | Severity | Enabled | Minutes | Tags | Scope |"
echo "|-----|------|----------|---------|---------|------|-------|"

for file in "$DIAG_PATH"/*Diagnostic.java; do
    filename=$(basename "$file")
    # Skip abstract classes
    if [[ $filename == Abstract* ]] || [[ $filename == BSLDiagnostic.java ]]; then
        continue
    fi

    # Extract diagnostic key (remove "Diagnostic.java" suffix)
    key="${filename%Diagnostic.java}"

    # Extract metadata annotation
    type=$(grep -A 20 "@DiagnosticMetadata" "$file" | grep "type =" | sed -E 's/.*DiagnosticType\.([A-Z_]+).*/\1/' | head -1)
    severity=$(grep -A 20 "@DiagnosticMetadata" "$file" | grep "severity =" | sed -E 's/.*DiagnosticSeverity\.([A-Z_]+).*/\1/' | head -1)
    activated=$(grep -A 20 "@DiagnosticMetadata" "$file" | grep "activatedByDefault" | sed -E 's/.*= *([a-z]+).*/\1/' | head -1)
    minutes=$(grep -A 20 "@DiagnosticMetadata" "$file" | grep "minutesToFix" | sed -E 's/.*= *([0-9]+).*/\1/' | head -1)
    tags=$(grep -A 20 "@DiagnosticMetadata" "$file" | grep -A 5 "tags =" | grep "DiagnosticTag\." | sed -E 's/.*DiagnosticTag\.([A-Z_]+).*/\1/' | tr '\n' ',' | sed 's/,$//')
    scope=$(grep -A 20 "@DiagnosticMetadata" "$file" | grep "scope =" | sed -E 's/.*DiagnosticScope\.([A-Z_]+).*/\1/' | head -1)

    # Set defaults
    [[ -z "$type" ]] && type="ERROR"
    [[ -z "$severity" ]] && severity="MINOR"
    [[ -z "$activated" ]] && activated="true"
    [[ -z "$minutes" ]] && minutes="0"
    [[ -z "$scope" ]] && scope="ALL"

    echo "| $key | $type | $severity | $activated | $minutes | $tags | $scope |"
done
