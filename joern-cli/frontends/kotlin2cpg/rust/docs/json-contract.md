# Kotlin Astgen JSON Contract

`kotlinastgen` emits one JSON document per parsed Kotlin source file.

```json
{
  "fullName": "/absolute/path/Sample.kt",
  "relativeName": "demo/Sample.kt",
  "ast": {
    "kind": "source_file",
    "fieldName": null,
    "named": true,
    "missing": false,
    "extra": false,
    "hasError": false,
    "startByte": 0,
    "endByte": 42,
    "start": { "line": 1, "column": 1 },
    "end": { "line": 3, "column": 1 },
    "code": "package demo\nclass Sample {}\n",
    "children": []
  }
}
```

Node `kind` values come from `tree-sitter-kotlin`. Byte offsets are UTF-8 byte
offsets into the source text. Lines and columns are one-based.
