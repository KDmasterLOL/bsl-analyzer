# Diagnostic Configuration Schema

## Overview

bsl-analyzer uses the `.bsl-language-server.json` configuration format.

Configuration file: `.bsl-language-server.json` (placed in project root)

---

## Complete Configuration Schema

```json
{
  "$schema": "http://json-schema.org/draft-07/schema#",
  "title": "BSL Language Server Configuration",
  "type": "object",
  "properties": {
    "language": {
      "description": "Interface language",
      "type": "string",
      "enum": ["ru", "en"],
      "default": "ru"
    },
    "diagnostics": {
      "description": "Diagnostic configuration",
      "type": "object",
      "properties": {
        "computeTrigger": {
          "description": "When to compute diagnostics",
          "type": "string",
          "enum": ["onSave", "onType"],
          "default": "onSave"
        },
        "skipSupport": {
          "description": "Skip support level",
          "type": "string",
          "enum": ["never", "withSupport", "withSupportLocked"],
          "default": "never"
        },
        "mode": {
          "description": "Diagnostic execution mode",
          "type": "string",
          "enum": ["on", "off", "only", "except"],
          "default": "on"
        },
        "parameters": {
          "description": "Diagnostic-specific parameters",
          "type": "object",
          "additionalProperties": true
        }
      }
    },
    "codeLens": {
      "description": "Code lens configuration",
      "type": "object",
      "properties": {
        "parameters": {
          "type": "object",
          "properties": {
            "cognitiveComplexity": {"type": "boolean", "default": false},
            "cyclomaticComplexity": {"type": "boolean", "default": false}
          }
        }
      }
    },
    "configurationRoot": {
      "description": "Path to 1C configuration root (Configuration.xml)",
      "type": "string"
    }
  }
}
```

---

## Diagnostic Modes

### Mode: `on` (default)

Run all diagnostics enabled by default.

```json
{
  "diagnostics": {
    "mode": "on"
  }
}
```

---

### Mode: `off`

Disable all diagnostics.

```json
{
  "diagnostics": {
    "mode": "off"
  }
}
```

---

### Mode: `only`

Run **only** specified diagnostics.

```json
{
  "diagnostics": {
    "mode": "only",
    "parameters": {
      "LineLength": true,
      "MethodSize": true,
      "CyclomaticComplexity": {
        "complexityThreshold": 15
      }
    }
  }
}
```

---

### Mode: `except`

Run all diagnostics **except** specified ones.

```json
{
  "diagnostics": {
    "mode": "except",
    "parameters": {
      "CommentedCode": false,
      "MagicNumber": false
    }
  }
}
```

---

## Diagnostic Parameters

Each diagnostic can be:
1. **Disabled:** Set to `false`
2. **Enabled with defaults:** Set to `true` or omit
3. **Configured:** Set to object with parameters

### Disable Diagnostic

```json
{
  "diagnostics": {
    "parameters": {
      "MethodSize": false
    }
  }
}
```

---

### Configure Diagnostic

```json
{
  "diagnostics": {
    "parameters": {
      "LineLength": {
        "maxLineLength": 140
      }
    }
  }
}
```

---

## Global Analyzer Parameters

### dataflow_max_iterations

**Default:** 10000 iterations

**Description:** Maximum iterations for dataflow analysis (liveness, reaching definitions, etc.).

Controls convergence limit for dataflow algorithms used by diagnostics like `UnusedLocalVariable`.
Increase this for extremely complex methods with deep nesting or many loops.
Warning is logged if analysis exceeds this limit.

**Configuration:**

```json
{
  "diagnostics": {
    "dataflow_max_iterations": 20000
  }
}
```

**When to increase:**
- Methods with extremely deep nesting (>15 levels)
- Methods with very many loops (>30 loops)
- Highly complex control flow (many nested `Если`/`Пока`/`Для` combinations)
- Warning message: `WARN Backward dataflow analysis exceeded max iterations: N iterations`

**Note:** Higher values increase analysis time but improve accuracy for complex code. Default of 10000 handles most real-world code.

---

## Common Diagnostic Parameters

### LineLength

**Default:** 120 characters

```json
{
  "diagnostics": {
    "parameters": {
      "LineLength": {
        "maxLineLength": 140
      }
    }
  }
}
```

---

### MethodSize

**Default:** 200 lines

```json
{
  "diagnostics": {
    "parameters": {
      "MethodSize": {
        "maxMethodSize": 250
      }
    }
  }
}
```

---

### CyclomaticComplexity

**Defaults:**
- `complexityThreshold`: 20

```json
{
  "diagnostics": {
    "parameters": {
      "CyclomaticComplexity": {
        "complexityThreshold": 15
      }
    }
  }
}
```

---

### CognitiveComplexity

**Default:** 15

```json
{
  "diagnostics": {
    "parameters": {
      "CognitiveComplexity": {
        "complexityThreshold": 12
      }
    }
  }
}
```

---

### NestedStatements

**Default:** 4 levels

```json
{
  "diagnostics": {
    "parameters": {
      "NestedStatements": {
        "maxAllowedLevel": 5
      }
    }
  }
}
```

---

### NumberOfParams

**Default:** 7 parameters

```json
{
  "diagnostics": {
    "parameters": {
      "NumberOfParams": {
        "maxParamsCount": 5
      }
    }
  }
}
```

---

### NumberOfOptionalParams

**Default:** 3 parameters

```json
{
  "diagnostics": {
    "parameters": {
      "NumberOfOptionalParams": {
        "maxOptionalParamsCount": 2
      }
    }
  }
}
```

---

### NumberOfValuesInStructureConstructor

**Default:** 3 values

```json
{
  "diagnostics": {
    "parameters": {
      "NumberOfValuesInStructureConstructor": {
        "maxValuesCount": 5
      }
    }
  }
}
```

---

### TooManyReturns

**Default:** 3 returns
**Enabled by default:** ❌ No

```json
{
  "diagnostics": {
    "parameters": {
      "TooManyReturns": {
        "maxReturnsCount": 5
      }
    }
  }
}
```

---

### IfConditionComplexity

**Default:** 3 boolean operators

```json
{
  "diagnostics": {
    "parameters": {
      "IfConditionComplexity": {
        "maxIfConditionComplexity": 5
      }
    }
  }
}
```

---

### MagicNumber

**Defaults:**
- Allow: -1, 0, 1
- Configurable authorized numbers

```json
{
  "diagnostics": {
    "parameters": {
      "MagicNumber": {
        "authorizedNumbers": "-1,0,1,2,10,100,1000"
      }
    }
  }
}
```

---

### MagicDate

**Configurable authorized dates:**

```json
{
  "diagnostics": {
    "parameters": {
      "MagicDate": {
        "authorizedDates": "00010101,00010101000000"
      }
    }
  }
}
```

---

### BadWords

**Enabled by default:** ❌ No
**Requires word list:**

```json
{
  "diagnostics": {
    "parameters": {
      "BadWords": {
        "words": "хрень,дурацкий,тупой,костыль"
      }
    }
  }
}
```

---

### CommentedCode

**Default threshold:** 0.9 (90% code confidence)

```json
{
  "diagnostics": {
    "parameters": {
      "CommentedCode": {
        "threshold": 0.85
      }
    }
  }
}
```

---

### ConsecutiveEmptyLines

**Default:** 1 empty line allowed

```json
{
  "diagnostics": {
    "parameters": {
      "ConsecutiveEmptyLines": {
        "allowedEmptyLinesCount": 2
      }
    }
  }
}
```

---

### DuplicateStringLiteral

**Defaults:**
- `minLength`: 50 characters
- `threshold`: 2 occurrences

```json
{
  "diagnostics": {
    "parameters": {
      "DuplicateStringLiteral": {
        "minLength": 100,
        "threshold": 3
      }
    }
  }
}
```

---

## Complete Example Configuration

```json
{
  "language": "en",
  "configurationRoot": "src/cf",
  "diagnostics": {
    "computeTrigger": "onType",
    "skipSupport": "withSupportLocked",
    "mode": "on",
    "parameters": {
      // Disable specific diagnostics
      "CommentedCode": false,
      "MagicNumber": false,

      // Configure thresholds
      "LineLength": {
        "maxLineLength": 140
      },
      "MethodSize": {
        "maxMethodSize": 250
      },
      "CyclomaticComplexity": {
        "complexityThreshold": 15
      },
      "CognitiveComplexity": {
        "complexityThreshold": 12
      },
      "NestedStatements": {
        "maxAllowedLevel": 5
      },
      "NumberOfParams": {
        "maxParamsCount": 5
      },
      "IfConditionComplexity": {
        "maxIfConditionComplexity": 4
      },

      // Enable disabled-by-default diagnostics
      "BadWords": {
        "words": "хрень,костыль,тупой"
      },
      "TooManyReturns": {
        "maxReturnsCount": 4
      }
    }
  },
  "codeLens": {
    "parameters": {
      "cognitiveComplexity": true,
      "cyclomaticComplexity": true
    }
  }
}
```

---

## Environment-Specific Configurations

### Development (Strict)

```json
{
  "diagnostics": {
    "mode": "on",
    "parameters": {
      "LineLength": {"maxLineLength": 120},
      "MethodSize": {"maxMethodSize": 150},
      "CyclomaticComplexity": {"complexityThreshold": 10},
      "CognitiveComplexity": {"complexityThreshold": 10}
    }
  }
}
```

---

### Legacy Code (Relaxed)

```json
{
  "diagnostics": {
    "mode": "except",
    "parameters": {
      "MagicNumber": false,
      "CommentedCode": false,
      "MethodSize": false,
      "CyclomaticComplexity": false,
      "CognitiveComplexity": false
    }
  }
}
```

---

### Security Audit (Security Focus)

```json
{
  "diagnostics": {
    "mode": "only",
    "parameters": {
      "DisableSafeMode": true,
      "ExecuteExternalCode": true,
      "ExecuteExternalCodeInCommonModule": true,
      "ExternalAppStarting": true,
      "FileSystemAccess": true,
      "InternetAccess": true,
      "OSUsersMethod": true,
      "PrivilegedModuleMethodCall": true,
      "SetPermissionsForNewObjects": true,
      "SetPrivilegedMode": true,
      "UsingExternalCodeTools": true,
      "UsingHardcodeNetworkAddress": true,
      "UsingHardcodePath": true,
      "UsingHardcodeSecretInformation": true,
      "UseSystemInformation": true
    }
  }
}
```

---

### Performance Audit (Performance Focus)

```json
{
  "diagnostics": {
    "mode": "only",
    "parameters": {
      "CreateQueryInCycle": true,
      "DeletingCollectionItem": true,
      "FieldsFromJoinsWithoutIsNull": true,
      "FullOuterJoinQuery": true,
      "JoinWithSubQuery": true,
      "JoinWithVirtualTable": true,
      "LogicalOrInJoinQuerySection": true,
      "LogicalOrInTheWhereSectionOfQuery": true,
      "MissingTempStorageDeletion": true,
      "QueryNestedFieldsByDot": true,
      "RefOveruse": true,
      "SelectTopWithoutOrderBy": true,
      "TransferringParametersBetweenClientAndServer": true,
      "UsingFindElementByString": true,
      "VirtualTableCallWithoutParameters": true
    }
  }
}
```

---

## Configuration Discovery

bsl-analyzer searches for `.bsl-language-server.json` in this order:

1. Current working directory
2. Project root (git repository root)
3. Parent directories (up to filesystem root)
4. User home directory (`~/.bsl-language-server.json`)

**Recommendation:** Place in project root alongside `.git/`

---

## Using existing configuration

✅ **No changes required!** Existing `.bsl-language-server.json` files work without modification.

---

## Validation

bsl-analyzer validates configuration on startup:

- Unknown diagnostics → **Warning** (ignored)
- Invalid parameters → **Error** (use defaults)
- Invalid JSON → **Error** (use defaults)

Run `bsl-analyzer validate-config` to check configuration:

```bash
bsl-analyzer validate-config .bsl-language-server.json
```

