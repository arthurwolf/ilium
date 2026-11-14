---
layout: page
title: Manifest schema and error catalogue
permalink: /pages/reference-manifest-schema/
---

## Manifest schema and error catalogue

This page holds the machine-readable schema of `manifest.json` and the full list of error codes that the package, signing, registry, dependency and install steps can print. The schema is a JSON Schema document for the draft 2020-12. The catalogue gives, for each code, the stage that raises it, the exit code of the command and the message template. The text of every key is in the [Manifest reference](/pages/reference-manifest/).

Requirement ids such as `PKG-560` can be cited by tests. See [Requirements: manifest and package](/pages/reference-requirements-manifest-package/).

### You will need

- The validation stages: [Package format](/pages/reference-package-format/#the-validation-order).
- The key tables, limits and rules: [Manifest reference](/pages/reference-manifest/).
- The output records and exit codes: [Plugin command line](/pages/reference-plugin-cli/) and [Distribution commands](/pages/reference-plugin-distribution-cli/#exit-codes).

## The schema file

The schema describes a source manifest and a packed manifest. In a source manifest `files` may be missing. The schema accepts both, and stage 3 requires `files` in a packed manifest (`manifest.key-missing`).

| Id | Rule |
|---|---|
| [PKG-560] | The schema document MUST declare `$schema` as `https://json-schema.org/draft/2020-12/schema` and `$id` as `urn:ilium:schema:plugin-manifest:0.1.0`. It MUST NOT use a keyword outside the 2020-12 core and validation vocabularies. |
| [PKG-561] | `ilium plugin check` and the installer MUST accept exactly the manifests that this schema accepts at stage 3, and MUST apply the extra rules of the list below at stages 3 and 4. A manifest that the schema refuses MUST be refused with the code of the failing key (see the mapping in PKG-562). |
| [PKG-562] | A schema failure maps to a code in this order: a property outside `properties` gives `manifest.unknown-key`; a missing `required` property gives `manifest.key-missing`; a failing `pattern` on `id` gives `manifest.id-invalid`; a failing `pattern` on `version` gives `manifest.version-invalid`; a failing `pattern` on `name`, `publisher` or `description` gives `manifest.display-name`; a failing `data_version` gives `manifest.data-version`; every other failure gives `manifest.schema`. |
| [PKG-563] | The schema MUST NOT be fetched from the network. A copy ships with the host and with `ilium plugin check`, and `ilium plugin check --print-schema` writes it to standard output. |

```json
{
  "$schema": "https://json-schema.org/draft/2020-12/schema",
  "$id": "urn:ilium:schema:plugin-manifest:0.1.0",
  "title": "Ilium plugin manifest, contract ilium:plugin@0.1.0",
  "type": "object",
  "additionalProperties": false,
  "required": ["api", "id", "name", "version", "publisher"],
  "anyOf": [{"required": ["server"]}, {"required": ["client"]}, {"required": ["scene"]}],
  "properties": {
    "api": {"const": "ilium:plugin@0.1.0"},
    "id": {
      "type": "string",
      "maxLength": 128,
      "pattern": "^[a-z0-9]([a-z0-9-]{0,30}[a-z0-9])?(\\.[a-z0-9]([a-z0-9-]{0,30}[a-z0-9])?){1,7}$"
    },
    "name": {"type": "string", "minLength": 1, "maxLength": 64, "pattern": "^[^\\u0000-\\u001f\\u007f-\\u009f  ‪-‮⁦-⁩]+$"},
    "version": {
      "type": "string",
      "maxLength": 64,
      "pattern": "^(0|[1-9][0-9]*)\\.(0|[1-9][0-9]*)\\.(0|[1-9][0-9]*)(-((0|[1-9][0-9]*|[0-9]*[A-Za-z-][0-9A-Za-z-]*)(\\.(0|[1-9][0-9]*|[0-9]*[A-Za-z-][0-9A-Za-z-]*))*))?(\\+[0-9A-Za-z-]+(\\.[0-9A-Za-z-]+)*)?$"
    },
    "publisher": {"type": "string", "minLength": 1, "maxLength": 64, "pattern": "^[^\\u0000-\\u001f\\u007f-\\u009f  ‪-‮⁦-⁩]+$"},
    "description": {"type": "string", "maxLength": 500, "pattern": "^[^\\u0000-\\u0009\\u000b-\\u001f\\u007f-\\u009f  ‪-‮⁦-⁩]*$"},
    "license": {"type": "string", "minLength": 1, "maxLength": 128},
    "icon": {"type": "string", "minLength": 1, "maxLength": 64, "pattern": "^[a-z0-9]([a-z0-9-]*[a-z0-9])?$"},
    "categories": {
      "type": "array",
      "minItems": 1,
      "maxItems": 3,
      "uniqueItems": true,
      "items": {
        "enum": [
          "agents",
          "appearance",
          "scenes",
          "editors",
          "git",
          "inference",
          "notifications",
          "search",
          "terminal",
          "tools",
          "voice"
        ]
      }
    },
    "keywords": {
      "type": "array",
      "maxItems": 8,
      "uniqueItems": true,
      "items": {"type": "string", "minLength": 1, "maxLength": 24, "pattern": "^[^\\s]+$"}
    },
    "homepage": {"type": "string", "maxLength": 256, "pattern": "^https://[^\\s]+$"},
    "repository": {"type": "string", "maxLength": 256, "pattern": "^https://[^\\s]+$"},
    "data_use": {
      "type": "array",
      "maxItems": 8,
      "items": {
        "type": "object",
        "additionalProperties": false,
        "required": ["what", "where", "purpose", "opt_in"],
        "properties": {
          "what": {"type": "string", "minLength": 1, "maxLength": 200},
          "where": {"type": "string", "minLength": 1, "maxLength": 200},
          "purpose": {"type": "string", "minLength": 1, "maxLength": 200},
          "opt_in": {"type": "boolean"}
        }
      }
    },
    "security_contact": {"type": "string", "minLength": 1, "maxLength": 200},
    "support_url": {"type": "string", "maxLength": 256, "pattern": "^https://[^\\s]+$"},
    "imports_from": {"type": "array", "maxItems": 4, "uniqueItems": true, "items": {"type": "string", "maxLength": 128, "pattern": "^[a-z0-9]([a-z0-9-]{0,30}[a-z0-9])?(\\.[a-z0-9]([a-z0-9-]{0,30}[a-z0-9])?){1,7}$"}},
    "accessibility": {
      "type": "object",
      "additionalProperties": false,
      "properties": {
        "screen_reader": {"enum": ["none", "partial", "full"]},
        "high_contrast": {"enum": ["none", "partial", "full"]},
        "reduced_motion": {"enum": ["none", "partial", "full"]},
        "compact_40_columns": {"enum": ["none", "partial", "full"]},
        "languages": {"type": "array", "maxItems": 64, "uniqueItems": true, "items": {"type": "string", "minLength": 2, "maxLength": 35}}
      }
    },
    "server": {
      "type": "object",
      "additionalProperties": false,
      "required": ["entry", "world"],
      "properties": {
        "entry": {"const": "server.wasm"},
        "world": {"const": "ilium:server/base@0.1.0"},
        "exports": {
          "type": "array",
          "uniqueItems": true,
          "maxItems": 9,
          "items": {
            "enum": [
              "detector",
              "job-runner",
              "service-provider",
              "inference-provider",
              "settings-validate",
              "catalog-provider",
              "virtual-rows-provider",
              "hook-handler",
              "migrator"
            ]
          }
        }
      }
    },
    "client": {
      "type": "object",
      "additionalProperties": false,
      "required": ["entry", "world"],
      "properties": {
        "entry": {"const": "client.wasm"},
        "world": {"const": "ilium:client/base@0.1.0"},
        "exports": {
          "type": "array",
          "uniqueItems": true,
          "maxItems": 12,
          "items": {
            "enum": [
              "views",
              "annotator",
              "job-runner",
              "service-provider",
              "inference-provider",
              "settings-validate",
              "catalog-provider",
              "menu-provider",
              "virtual-rows-provider",
              "token-provider",
              "hook-handler",
              "migrator"
            ]
          }
        }
      }
    },
    "scene": {
      "type": "object",
      "additionalProperties": false,
      "required": ["entry", "world"],
      "properties": {
        "entry": {"const": "scene.wasm"},
        "world": {"const": "ilium:anim/base@0.1.0"},
        "exports": {"type": "array", "uniqueItems": true, "maxItems": 1, "items": {"enum": ["job-runner"]}}
      }
    },
    "capabilities": {
      "type": "object",
      "additionalProperties": false,
      "properties": {
        "server": {"type": "array", "uniqueItems": true, "maxItems": 256, "items": {"$ref": "#/$defs/grant"}},
        "client": {"type": "array", "uniqueItems": true, "maxItems": 256, "items": {"$ref": "#/$defs/grant"}},
        "scene": {"type": "array", "uniqueItems": true, "maxItems": 256, "items": {"$ref": "#/$defs/grant"}}
      }
    },
    "activation": {
      "type": "array",
      "uniqueItems": true,
      "maxItems": 32,
      "items": {
        "type": "string",
        "maxLength": 256,
        "pattern": "^(startup|durable-timer|(event|command|view|pane-kind|scene|cli|http|service|topic|settings|catalog|menu|virtual-rows|language|client-request):[^\\s]+)$"
      }
    },
    "requires": {
      "type": "object",
      "additionalProperties": false,
      "properties": {
        "host": {"type": "string", "minLength": 1, "maxLength": 128},
        "services": {"type": "array", "items": {"type": "string", "minLength": 1, "maxLength": 300, "pattern": "^[^@\\s]+@(0|[1-9][0-9]*)\\.(0|[1-9][0-9]*)\\.(0|[1-9][0-9]*)$"}, "maxItems": 32},
        "plugins": {
          "type": "array",
          "items": {
            "type": "object",
            "additionalProperties": false,
            "properties": {
              "id": {
                "type": "string",
                "maxLength": 128,
                "pattern": "^[a-z0-9]([a-z0-9-]{0,30}[a-z0-9])?(\\.[a-z0-9]([a-z0-9-]{0,30}[a-z0-9])?){1,7}$"
              },
              "version": {"type": "string", "minLength": 1, "maxLength": 128},
              "optional": {"type": "boolean"}
            },
            "required": ["id"]
          },
          "maxItems": 32
        },
        "wait_ms": {"type": "integer", "minimum": 0, "maximum": 5000}
      }
    },
    "accepts_messages_from": {
      "type": "array",
      "maxItems": 32,
      "uniqueItems": true,
      "items": {"type": "string", "maxLength": 128}
    },
    "replaces": {
      "type": "array",
      "maxItems": 4,
      "uniqueItems": true,
      "items": {
        "type": "string",
        "maxLength": 128,
        "pattern": "^[a-z0-9]([a-z0-9-]{0,30}[a-z0-9])?(\\.[a-z0-9]([a-z0-9-]{0,30}[a-z0-9])?){1,7}$"
      }
    },
    "replaceable": {"type": "boolean"},
    "surface": {
      "type": "array",
      "minItems": 1,
      "maxItems": 64,
      "uniqueItems": true,
      "items": {"type": "string", "pattern": "^(services|points|commands|pane-kinds|topics|cli):[^\\s]+$"}
    },
    "contributes": {
      "type": "object",
      "additionalProperties": false,
      "properties": {
        "commands": {
          "type": "array",
          "items": {
            "type": "object",
            "additionalProperties": false,
            "properties": {
              "id": {"type": "string", "minLength": 1, "maxLength": 64, "pattern": "^[a-z0-9]([a-z0-9-]*[a-z0-9])?$"},
              "title": {"type": "string", "minLength": 1, "maxLength": 64},
              "palette": {"type": "boolean"},
              "voice": {"type": "array", "items": {"type": "string", "minLength": 1, "maxLength": 64}, "maxItems": 8},
              "arguments_schema": {"type": "object"},
              "icon": {"type": "string", "minLength": 1, "maxLength": 64},
              "when": {"type": "string", "minLength": 1, "maxLength": 256},
              "side": {"enum": ["server", "client"]},
              "category": {"type": "string", "minLength": 1, "maxLength": 32},
              "keywords": {"type": "array", "items": {"type": "string", "minLength": 1, "maxLength": 24}, "maxItems": 8}
            },
            "required": ["id", "title"]
          },
          "maxItems": 256
        },
        "keybindings": {
          "type": "array",
          "items": {
            "type": "object",
            "additionalProperties": false,
            "properties": {
              "command": {"type": "string", "minLength": 1, "maxLength": 128},
              "chord": {"type": "string", "minLength": 1, "maxLength": 64},
              "mode": {"type": "string", "minLength": 1, "maxLength": 64}
            },
            "required": ["command", "chord"]
          },
          "maxItems": 256
        },
        "settings": {
          "type": "array",
          "items": {
            "type": "object",
            "additionalProperties": false,
            "properties": {
              "kind": {"enum": ["tab", "section"]},
              "id": {"type": "string", "minLength": 1, "maxLength": 32, "pattern": "^[a-z][a-z0-9-]{0,31}$"},
              "title": {"type": "string", "minLength": 1, "maxLength": 48},
              "short_title": {"type": "string", "minLength": 1, "maxLength": 16},
              "icon": {"type": "string", "minLength": 1, "maxLength": 64},
              "glyph": {"type": "string", "minLength": 1, "maxLength": 4},
              "after": {"type": "string", "minLength": 1, "maxLength": 64},
              "order": {"type": "integer", "minimum": 0, "maximum": 1000},
              "render": {"enum": ["rows", "view"]},
              "keywords": {"type": "array", "items": {"type": "string", "minLength": 1, "maxLength": 24}, "maxItems": 16},
              "sections": {"type": "array", "items": {"type": "object"}, "maxItems": 64},
              "tab": {"type": "string", "minLength": 1, "maxLength": 64},
              "placement": {"type": "string", "minLength": 1, "maxLength": 64},
              "scope": {"enum": ["server", "client"]},
              "rows": {"type": "array", "items": {"type": "object"}, "maxItems": 1024},
              "collapsed": {"type": "boolean"}
            },
            "required": ["kind", "id"]
          },
          "maxItems": 64
        },
        "help": {
          "type": "array",
          "items": {
            "type": "object",
            "additionalProperties": false,
            "properties": {
              "id": {"type": "string", "minLength": 1, "maxLength": 64, "pattern": "^[a-z0-9]([a-z0-9-]*[a-z0-9])?$"},
              "file": {"type": "string", "pattern": "^help/[A-Za-z0-9._-]+\\.md$"},
              "title": {"type": "string", "minLength": 1, "maxLength": 64}
            },
            "required": ["id", "file", "title"]
          },
          "maxItems": 64
        },
        "decoration_slots": {
          "type": "array",
          "items": {
            "type": "object",
            "additionalProperties": false,
            "properties": {
              "id": {"type": "string", "minLength": 1, "maxLength": 64, "pattern": "^[a-z0-9]([a-z0-9-]*[a-z0-9])?$"},
              "placement": {"enum": ["badge", "icon", "subtitle", "footer"]},
              "priority": {"type": "integer", "minimum": 0, "maximum": 100},
              "visibility": {"enum": ["always", "on-hover"]},
              "trailing": {"type": "boolean"}
            },
            "required": ["id", "placement"]
          },
          "maxItems": 16
        },
        "virtual_rows": {
          "type": "array",
          "items": {
            "type": "object",
            "additionalProperties": false,
            "properties": {
              "id": {"type": "string", "minLength": 1, "maxLength": 64, "pattern": "^[a-z0-9]([a-z0-9-]*[a-z0-9])?$"},
              "title": {"type": "string", "minLength": 1, "maxLength": 48},
              "icon": {"type": "string", "minLength": 1, "maxLength": 64},
              "parent": {"enum": ["project", "session"]},
              "expandable": {"type": "boolean"},
              "children": {"enum": ["static", "provider"]},
              "accepts_drop": {"type": "array", "items": {"enum": ["pane", "file", "text"]}, "maxItems": 3},
              "drop_command": {"type": "string", "minLength": 1, "maxLength": 128}
            },
            "required": ["id", "title", "parent"]
          },
          "maxItems": 16
        },
        "toolbar_buttons": {
          "type": "array",
          "items": {
            "type": "object",
            "additionalProperties": false,
            "properties": {
              "id": {"type": "string", "minLength": 1, "maxLength": 64, "pattern": "^[a-z0-9]([a-z0-9-]*[a-z0-9])?$"},
              "icon": {"type": "string", "minLength": 1, "maxLength": 64},
              "label": {"type": "string", "minLength": 1, "maxLength": 24},
              "command": {"type": "string", "minLength": 1, "maxLength": 128},
              "applies_to": {},
              "requires_action": {"type": "string", "minLength": 1, "maxLength": 64},
              "submenu": {},
              "right_click_command": {"type": "string", "minLength": 1, "maxLength": 128}
            },
            "required": ["id", "icon", "command"]
          },
          "maxItems": 16
        },
        "context_menu": {
          "type": "array",
          "items": {
            "type": "object",
            "additionalProperties": false,
            "properties": {
              "id": {"type": "string", "minLength": 1, "maxLength": 64, "pattern": "^[a-z0-9]([a-z0-9-]*[a-z0-9])?$"},
              "title": {"type": "string", "minLength": 1, "maxLength": 48},
              "command": {"type": "string", "minLength": 1, "maxLength": 128},
              "target": {"type": "string", "minLength": 1, "maxLength": 64},
              "icon": {"type": "string", "minLength": 1, "maxLength": 64},
              "when": {"type": "string", "minLength": 1, "maxLength": 256},
              "dynamic": {"type": "boolean"},
              "items": {"type": "array", "items": {"type": "object"}, "maxItems": 64}
            },
            "required": ["id", "title", "target"]
          },
          "maxItems": 16
        },
        "panels": {
          "type": "array",
          "items": {
            "type": "object",
            "additionalProperties": false,
            "properties": {
              "id": {"type": "string", "minLength": 1, "maxLength": 64, "pattern": "^[a-z0-9]([a-z0-9-]*[a-z0-9])?$"},
              "title": {"type": "string", "minLength": 1, "maxLength": 48},
              "icon": {"type": "string", "minLength": 1, "maxLength": 64},
              "default_size": {},
              "modal": {"type": "boolean"},
              "placement": {"enum": ["overlay", "left", "right", "bottom"]},
              "min_size": {},
              "mode_label": {"type": "string", "minLength": 1, "maxLength": 16},
              "wants_keys": {"type": "boolean"}
            },
            "required": ["id", "title"]
          },
          "maxItems": 16
        },
        "pane_kinds": {
          "type": "array",
          "items": {
            "type": "object",
            "additionalProperties": false,
            "properties": {
              "id": {"type": "string", "minLength": 1, "maxLength": 64, "pattern": "^[a-z0-9]([a-z0-9-]*[a-z0-9])?$"},
              "title": {"type": "string", "minLength": 1, "maxLength": 48},
              "icon": {"type": "string", "minLength": 1, "maxLength": 64},
              "primitive": {"enum": ["basic"]},
              "wants_keys": {"type": "boolean"},
              "persist": {"enum": ["metadata", "none"]}
            },
            "required": ["id", "title"]
          },
          "maxItems": 16
        },
        "scenes": {
          "type": "array",
          "items": {
            "type": "object",
            "additionalProperties": false,
            "properties": {
              "id": {"type": "string", "minLength": 1, "maxLength": 64, "pattern": "^[a-z0-9]([a-z0-9-]*[a-z0-9])?$"},
              "title": {"type": "string", "minLength": 1, "maxLength": 48},
              "family": {"type": "string", "minLength": 1, "maxLength": 32},
              "modes": {},
              "settings_schema": {"type": "object"},
              "frame_rate_cap": {"type": "integer", "minimum": 1, "maximum": 60},
              "prerender": {"type": "object"},
              "clip_seconds": {"type": "integer", "minimum": 1, "maximum": 60},
              "glyph_plane": {"type": "boolean"}
            },
            "required": ["id", "title"]
          },
          "maxItems": 16
        },
        "signatures": {
          "type": "array",
          "items": {
            "type": "object",
            "additionalProperties": false,
            "properties": {
              "id": {"type": "string", "minLength": 1, "maxLength": 64, "pattern": "^[a-z0-9]([a-z0-9-]*[a-z0-9])?$"},
              "process_names": {"type": "array", "items": {"type": "string", "minLength": 1, "maxLength": 255}, "maxItems": 32},
              "aliases": {"type": "array", "items": {"type": "string", "minLength": 1, "maxLength": 255}, "maxItems": 32},
              "icon": {"type": "string", "minLength": 1, "maxLength": 64},
              "resume": {"type": "array", "items": {"type": "string", "minLength": 1, "maxLength": 512}, "maxItems": 32},
              "actions": {"type": "object", "maxProperties": 32, "propertyNames": {"maxLength": 64, "pattern": "^[a-z0-9]([a-z0-9-]*[a-z0-9])?$"}, "additionalProperties": {"type": "object", "additionalProperties": false, "properties": {"keys": {"type": "string", "minLength": 1, "maxLength": 1024}, "allowed_when": {"type": "array", "uniqueItems": true, "maxItems": 8, "items": {"type": "string", "minLength": 1, "maxLength": 32}}, "stages": {"type": "array", "minItems": 1, "maxItems": 16, "items": {"type": "object"}}}}},
              "submit_keys": {"type": "string", "minLength": 1, "maxLength": 64},
              "session_scope": {"type": "string", "minLength": 1, "maxLength": 32}
            },
            "required": ["id", "process_names"]
          },
          "maxItems": 16
        },
        "output_filters": {
          "type": "array",
          "items": {
            "type": "object",
            "additionalProperties": false,
            "properties": {
              "id": {"type": "string", "minLength": 1, "maxLength": 64, "pattern": "^[a-z0-9]([a-z0-9-]*[a-z0-9])?$"},
              "regex": {"type": "string", "minLength": 1, "maxLength": 512},
              "panes": {"enum": ["all", "agents", "terminals"]},
              "rate_per_second": {"type": "integer", "minimum": 1, "maximum": 100}
            },
            "required": ["id", "regex"]
          },
          "maxItems": 32
        },
        "link_detectors": {
          "type": "array",
          "items": {
            "type": "object",
            "additionalProperties": false,
            "properties": {
              "id": {"type": "string", "minLength": 1, "maxLength": 64, "pattern": "^[a-z0-9]([a-z0-9-]*[a-z0-9])?$"},
              "regex": {"type": "string", "minLength": 1, "maxLength": 512},
              "command": {"type": "string", "minLength": 1, "maxLength": 128}
            },
            "required": ["id", "regex", "command"]
          },
          "maxItems": 16
        },
        "editor_line_actions": {
          "type": "array",
          "items": {
            "type": "object",
            "additionalProperties": false,
            "properties": {
              "id": {"type": "string", "minLength": 1, "maxLength": 64, "pattern": "^[a-z0-9]([a-z0-9-]*[a-z0-9])?$"},
              "title": {"type": "string", "minLength": 1, "maxLength": 48},
              "regex": {"type": "string", "minLength": 1, "maxLength": 512},
              "command": {"type": "string", "minLength": 1, "maxLength": 128}
            },
            "required": ["id", "title", "command"]
          },
          "maxItems": 16
        },
        "sounds": {
          "type": "array",
          "items": {
            "type": "object",
            "additionalProperties": false,
            "properties": {
              "id": {"type": "string", "minLength": 1, "maxLength": 64, "pattern": "^[a-z0-9]([a-z0-9-]*[a-z0-9])?$"},
              "file": {"type": "string", "pattern": "^sounds/[A-Za-z0-9._-]+\\.(wav|ogg)$"}
            },
            "required": ["id", "file"]
          },
          "maxItems": 32
        },
        "cli": {
          "type": "array",
          "items": {
            "type": "object",
            "additionalProperties": false,
            "properties": {
              "name": {"type": "string", "minLength": 1, "maxLength": 64, "pattern": "^[a-z0-9]([a-z0-9-]*[a-z0-9])?$"},
              "summary": {"type": "string", "minLength": 1, "maxLength": 120},
              "arguments_schema": {"type": "object"},
              "aliases": {"type": "array", "items": {"type": "string", "minLength": 1, "maxLength": 64, "pattern": "^[a-z0-9]([a-z0-9-]*[a-z0-9])?$"}},
              "output": {"enum": ["jsonl", "text", "mixed"]},
              "requires_server": {"type": "boolean"},
              "stdin": {"enum": ["none", "optional", "required"]},
              "argument_groups": {"type": "array", "items": {"enum": ["pane-selector"]}, "maxItems": 4}
            },
            "required": ["name", "summary"]
          },
          "maxItems": 16
        },
        "http_routes": {
          "type": "array",
          "items": {
            "type": "object",
            "additionalProperties": false,
            "properties": {
              "method": {"enum": ["GET", "POST", "PUT", "PATCH", "DELETE"]},
              "path": {"type": "string", "pattern": "^/", "maxLength": 256}
            },
            "required": ["method", "path"]
          },
          "maxItems": 32
        },
        "services": {
          "type": "object",
          "additionalProperties": false,
          "properties": {
            "provides": {
              "type": "array",
              "maxItems": 16,
              "items": {
                "type": "object",
                "additionalProperties": false,
                "properties": {
                  "name": {"type": "string", "minLength": 1, "maxLength": 256},
                  "version": {"type": "string", "minLength": 5, "maxLength": 64},
                  "schema": {"type": "string", "minLength": 1, "maxLength": 256}
                },
                "required": ["name", "version", "schema"]
              }
            }
          }
        },
        "topics": {
          "type": "object",
          "additionalProperties": false,
          "properties": {
            "publishes": {"type": "array", "items": {"type": "string", "minLength": 1, "maxLength": 256}, "maxItems": 64},
            "subscribes": {"type": "array", "items": {"type": "string", "minLength": 1, "maxLength": 256}, "maxItems": 64}
          }
        },
        "jobs": {
          "type": "array",
          "items": {
            "type": "object",
            "additionalProperties": false,
            "properties": {
              "kind": {"type": "string", "minLength": 1, "maxLength": 64, "pattern": "^[a-z0-9]([a-z0-9-]*[a-z0-9])?$"},
              "deadline_ms": {"type": "integer", "minimum": 1},
              "memory_mib": {"type": "integer", "minimum": 1}
            },
            "required": ["kind"]
          },
          "maxItems": 32
        },
        "onboarding_steps": {
          "type": "array",
          "items": {
            "type": "object",
            "additionalProperties": false,
            "properties": {
              "id": {"type": "string", "minLength": 1, "maxLength": 64, "pattern": "^[a-z0-9]([a-z0-9-]*[a-z0-9])?$"},
              "title": {"type": "string", "minLength": 1, "maxLength": 64},
              "order": {"type": "integer", "minimum": 0, "maximum": 1000}
            },
            "required": ["id", "title"]
          },
          "maxItems": 8
        },
        "extension_points": {
          "type": "array",
          "items": {
            "type": "object",
            "additionalProperties": false,
            "properties": {
              "id": {"type": "string", "minLength": 1, "maxLength": 64, "pattern": "^[a-z0-9]([a-z0-9-]*[a-z0-9])?$"},
              "kind": {"enum": ["collect", "hook"]},
              "mode": {"enum": ["observe", "transform", "veto", "first"]},
              "title": {"type": "string", "minLength": 1, "maxLength": 64},
              "entry_schema": {"type": "object"},
              "payload_schema": {"type": "object"},
              "reply_schema": {"type": "object"},
              "risk": {"enum": ["standard", "high"]},
              "deadline_ms": {"type": "integer", "minimum": 5, "maximum": 50},
              "max_handlers": {"type": "integer", "minimum": 1, "maximum": 8},
              "max_entries": {"type": "integer", "minimum": 1, "maximum": 256},
              "dynamic": {"type": "boolean"},
              "on_failure": {"enum": ["pass", "refuse"]},
              "final_check": {"type": "boolean"},
              "version": {
                "type": "string",
                "pattern": "^(0|[1-9][0-9]*)\\.(0|[1-9][0-9]*)\\.(0|[1-9][0-9]*)(-((0|[1-9][0-9]*|[0-9]*[A-Za-z-][0-9A-Za-z-]*)(\\.(0|[1-9][0-9]*|[0-9]*[A-Za-z-][0-9A-Za-z-]*))*))?(\\+[0-9A-Za-z-]+(\\.[0-9A-Za-z-]+)*)?$"
              }
            },
            "required": ["id", "kind"]
          },
          "maxItems": 16
        },
        "extensions": {
          "type": "array",
          "items": {
            "type": "object",
            "additionalProperties": false,
            "properties": {
              "point": {"type": "string", "minLength": 1, "maxLength": 160},
              "id": {"type": "string", "minLength": 1, "maxLength": 64, "pattern": "^[a-z0-9]([a-z0-9-]*[a-z0-9])?$"},
              "priority": {"type": "integer", "minimum": 0, "maximum": 100},
              "entry": {"type": "object"},
              "handler": {"type": "string", "minLength": 1, "maxLength": 64},
              "guard": {"type": "boolean"}
            },
            "required": ["point", "id"]
          },
          "maxItems": 64
        },
        "tree_toolbar": {
          "type": "array",
          "items": {
            "type": "object",
            "additionalProperties": false,
            "properties": {
              "id": {"type": "string", "minLength": 1, "maxLength": 64, "pattern": "^[a-z0-9]([a-z0-9-]*[a-z0-9])?$"},
              "icon": {"type": "string", "minLength": 1, "maxLength": 64},
              "label": {"type": "string", "minLength": 1, "maxLength": 24},
              "command": {"type": "string", "minLength": 1, "maxLength": 128},
              "caption": {"type": "string", "minLength": 1, "maxLength": 12},
              "popup_view": {"type": "string", "minLength": 1, "maxLength": 64, "pattern": "^[a-z0-9]([a-z0-9-]*[a-z0-9])?$"},
              "when": {"type": "string", "minLength": 1, "maxLength": 256}
            },
            "required": ["id", "icon", "label"]
          },
          "maxItems": 4
        },
        "row_actions": {
          "type": "array",
          "items": {
            "type": "object",
            "additionalProperties": false,
            "properties": {
              "id": {"type": "string", "minLength": 1, "maxLength": 64, "pattern": "^[a-z0-9]([a-z0-9-]*[a-z0-9])?$"},
              "icon": {"type": "string", "minLength": 1, "maxLength": 64},
              "label": {"type": "string", "minLength": 1, "maxLength": 24},
              "command": {"type": "string", "minLength": 1, "maxLength": 128},
              "applies_to": {"type": "array", "items": {"type": "string", "minLength": 1, "maxLength": 64}, "maxItems": 16},
              "when": {"type": "string", "minLength": 1, "maxLength": 256},
              "order": {"type": "integer", "minimum": 0, "maximum": 1000}
            },
            "required": ["id", "icon", "command"]
          },
          "maxItems": 4
        },
        "tree_orderings": {
          "type": "array",
          "items": {
            "type": "object",
            "additionalProperties": false,
            "properties": {
              "id": {"type": "string", "minLength": 1, "maxLength": 64, "pattern": "^[a-z0-9]([a-z0-9-]*[a-z0-9])?$"},
              "title": {"type": "string", "minLength": 1, "maxLength": 32},
              "slot": {"type": "string", "minLength": 1, "maxLength": 64, "pattern": "^[a-z0-9]([a-z0-9-]*[a-z0-9])?$"}
            },
            "required": ["id", "title", "slot"]
          },
          "maxItems": 8
        },
        "status_segments": {
          "type": "array",
          "items": {
            "type": "object",
            "additionalProperties": false,
            "properties": {
              "id": {"type": "string", "minLength": 1, "maxLength": 64, "pattern": "^[a-z0-9]([a-z0-9-]*[a-z0-9])?$"},
              "priority": {"type": "integer", "minimum": 0, "maximum": 100},
              "max_width": {"type": "integer", "minimum": 4, "maximum": 40}
            },
            "required": ["id"]
          },
          "maxItems": 2
        },
        "pane_frame_slots": {
          "type": "array",
          "items": {
            "type": "object",
            "additionalProperties": false,
            "properties": {
              "id": {"type": "string", "minLength": 1, "maxLength": 64, "pattern": "^[a-z0-9]([a-z0-9-]*[a-z0-9])?$"},
              "placement": {"enum": ["header-icon", "header-text", "footer-row", "banner"]},
              "applies_to": {}
            },
            "required": ["id", "placement"]
          },
          "maxItems": 4
        },
        "display_modes": {
          "type": "array",
          "items": {
            "type": "object",
            "additionalProperties": false,
            "properties": {
              "id": {"type": "string", "minLength": 1, "maxLength": 64, "pattern": "^[a-z0-9]([a-z0-9-]*[a-z0-9])?$"},
              "title": {"type": "string", "minLength": 1, "maxLength": 32},
              "icon": {"type": "string", "minLength": 1, "maxLength": 64},
              "slot": {"type": "string", "minLength": 1, "maxLength": 64, "pattern": "^[a-z0-9]([a-z0-9-]*[a-z0-9])?$"},
              "hide_unmarked": {"type": "boolean"}
            },
            "required": ["id", "title", "slot"]
          },
          "maxItems": 2
        },
        "catalogs": {
          "type": "array",
          "items": {
            "type": "object",
            "additionalProperties": false,
            "properties": {
              "id": {"type": "string", "minLength": 1, "maxLength": 64, "pattern": "^[a-z0-9]([a-z0-9-]*[a-z0-9])?$"},
              "title": {"type": "string", "minLength": 1, "maxLength": 48},
              "item_kind": {"enum": ["text", "glyph", "swatch", "pattern"]}
            },
            "required": ["id", "title", "item_kind"]
          },
          "maxItems": 8
        },
        "client_requests": {
          "type": "array",
          "items": {
            "type": "object",
            "additionalProperties": false,
            "properties": {
              "id": {"type": "string", "minLength": 1, "maxLength": 64, "pattern": "^[a-z0-9]([a-z0-9-]*[a-z0-9])?$"},
              "timeout_ms": {"type": "integer", "minimum": 1, "maximum": 30000}
            },
            "required": ["id"]
          },
          "maxItems": 16
        },
        "languages": {
          "type": "array",
          "items": {
            "type": "object",
            "additionalProperties": false,
            "properties": {
              "id": {"type": "string", "minLength": 1, "maxLength": 64, "pattern": "^[a-z0-9]([a-z0-9-]*[a-z0-9])?$"},
              "title": {"type": "string", "minLength": 1, "maxLength": 48},
              "extensions": {"type": "array", "items": {"type": "string", "minLength": 1, "maxLength": 16}, "maxItems": 32},
              "file_names": {"type": "array", "items": {"type": "string", "minLength": 1, "maxLength": 64}, "maxItems": 32}
            },
            "required": ["id", "title"]
          },
          "maxItems": 8
        }
      }
    },
    "limits": {
      "type": "object",
      "additionalProperties": false,
      "properties": {
        "memory_mib": {"type": "integer", "minimum": 0, "maximum": 4294967295},
        "table_elements": {"type": "integer", "minimum": 0, "maximum": 4294967295},
        "event_deadline_ms": {"type": "integer", "minimum": 0, "maximum": 4294967295},
        "command_deadline_ms": {"type": "integer", "minimum": 0, "maximum": 4294967295},
        "render_deadline_ms": {"type": "integer", "minimum": 0, "maximum": 4294967295},
        "annotate_deadline_ms": {"type": "integer", "minimum": 0, "maximum": 4294967295},
        "frame_deadline_ms": {"type": "integer", "minimum": 0, "maximum": 4294967295},
        "detector_deadline_ms": {"type": "integer", "minimum": 0, "maximum": 50, "description": "per detector call; 0 = default"},
        "activate_deadline_ms": {"type": "integer", "minimum": 0, "maximum": 4294967295},
        "deactivate_deadline_ms": {"type": "integer", "minimum": 0, "maximum": 4294967295},
        "migrate_deadline_ms": {"type": "integer", "minimum": 0, "maximum": 60000, "description": "hard deadline of one migrate call; 0 = default"},
        "job_deadline_ms": {"type": "integer", "minimum": 0, "maximum": 4294967295},
        "job_wall_ms": {"type": "integer", "minimum": 0, "maximum": 4294967295},
        "job_memory_mib": {"type": "integer", "minimum": 0, "maximum": 4294967295},
        "job_io_mib": {"type": "integer", "minimum": 0, "maximum": 4294967295},
        "concurrent_jobs": {"type": "integer", "minimum": 0, "maximum": 4294967295},
        "job_queue": {"type": "integer", "minimum": 0, "maximum": 4294967295},
        "event_queue": {"type": "integer", "minimum": 0, "maximum": 4294967295},
        "pending_requests": {"type": "integer", "minimum": 0, "maximum": 4294967295},
        "timers": {"type": "integer", "minimum": 0, "maximum": 4294967295},
        "output_filters": {"type": "integer", "minimum": 0, "maximum": 4294967295},
        "watches": {"type": "integer", "minimum": 0, "maximum": 4294967295},
        "key_grabs": {"type": "integer", "minimum": 0, "maximum": 4294967295},
        "ui_tree_nodes": {"type": "integer", "minimum": 0, "maximum": 4294967295},
        "ui_tree_kib": {"type": "integer", "minimum": 0, "maximum": 4294967295},
        "decorations": {"type": "integer", "minimum": 0, "maximum": 4294967295},
        "node_metadata_kib": {"type": "integer", "minimum": 0, "maximum": 4294967295},
        "storage_bucket_mib": {"type": "integer", "minimum": 0, "maximum": 4294967295},
        "disk_mib": {"type": "integer", "minimum": 0, "maximum": 4294967295},
        "fs_read_all_mib": {"type": "integer", "minimum": 0, "maximum": 4294967295},
        "fs_write_mib": {"type": "integer", "minimum": 0, "maximum": 4294967295},
        "http_response_mib": {"type": "integer", "minimum": 0, "maximum": 4294967295},
        "process_output_mib": {"type": "integer", "minimum": 0, "maximum": 4294967295},
        "log_lines_per_second": {"type": "integer", "minimum": 0, "maximum": 4294967295},
        "open_videos": {"type": "integer", "minimum": 0, "maximum": 4294967295},
        "video_buffer_mib": {"type": "integer", "minimum": 0, "maximum": 4294967295},
        "audio_captures": {"type": "integer", "minimum": 0, "maximum": 4294967295},
        "http_streams": {"type": "integer", "minimum": 0, "maximum": 4294967295},
        "children": {"type": "integer", "minimum": 0, "maximum": 4294967295},
        "cpu_ms_per_s": {"type": "integer", "minimum": 10, "maximum": 1000},
        "durable_timers": {"type": "integer", "minimum": 0, "maximum": 4294967295},
        "vector_indexes": {"type": "integer", "minimum": 0, "maximum": 4294967295},
        "vector_entries": {"type": "integer", "minimum": 0, "maximum": 4294967295},
        "shared_cache_mib": {"type": "integer", "minimum": 0, "maximum": 4294967295},
        "journal_entries": {"type": "integer", "minimum": 0, "maximum": 4294967295},
        "toasts_pending": {"type": "integer", "minimum": 0, "maximum": 4294967295},
        "package_assets_mib": {"type": "integer", "minimum": 0, "maximum": 4294967295}
      }
    },
    "data_version": {"type": "integer", "minimum": 1, "maximum": 4294967295},
    "min_data_version": {"type": "integer", "minimum": 1, "maximum": 4294967295},
    "limits_tier": {"enum": ["standard", "first-party"]},
    "unstable": {
      "type": "array",
      "uniqueItems": true,
      "maxItems": 8,
      "items": {"enum": ["ui-calendar", "plugin-services", "gpu-kernels", "websocket"]}
    },
    "files": {
      "type": "array",
      "maxItems": 4096,
      "items": {
        "type": "object",
        "additionalProperties": false,
        "properties": {
          "path": {"type": "string", "minLength": 1, "maxLength": 240, "pattern": "^[A-Za-z0-9._-]+(/[A-Za-z0-9._-]+)*$"},
          "length": {"type": "integer", "minimum": 0, "maximum": 67108864},
          "sha256": {"type": "string", "pattern": "^[0-9a-f]{64}$"}
        },
        "required": ["path", "length", "sha256"]
      }
    }
  },
  "dependentRequired": {"surface": ["replaceable"]},
  "allOf": [
    {
      "if": {"required": ["replaceable"], "properties": {"replaceable": {"const": true}}},
      "then": {"required": ["surface"]}
    }
  ],
  "$defs": {
    "grant": {
      "type": "string",
      "minLength": 3,
      "maxLength": 256,
      "pattern": "^[a-z][a-z0-9-]*(\\.[a-z0-9-]+)*(:[^\\u0000-\\u001f\\u007f]+)?$"
    }
  }
}
```

The schema cannot express these rules. The installer checks them in code, with the codes shown:

| Id | Rule that the schema cannot state | Code |
|---|---|---|
| [PKG-564] | The `id` is not reserved (`ilium.` prefix, and the publisher ids of PKG-220). | `manifest.id-reserved` |
| [PKG-565] | The `exports` list of a component matches the exports of the real component. | `manifest.exports-mismatch` |
| [PKG-566] | Each grant exists in this API version, its argument follows the grammar and it is available on its side. | `manifest.grant-unknown` and the grant codes |
| [PKG-567] | `when` and `visible_when` expressions parse, and settings keys are unique. | `manifest.when-syntax`, `manifest.settings-key-clash` |
| [PKG-568] | Ids are unique inside each contribution list, and commands, topics and services follow the prefix rules of their pages. | the code of the contribution |
| [PKG-569] | `requires` ranges parse, and `requires.plugins` holds no duplicate and not the plugin itself. | `manifest.requires-invalid`, `manifest.requires-self` |
| [PKG-570] | The side of each declared component has its file in the archive, and each file in `help/` and `sounds/` named by a contribution exists. | `manifest.file-missing` |

### Keys added by later requirements

The schema above holds every manifest key that a page of this manual specifies. The keys in this table were added by later requirements. A settings row is an open object in the schema file; its keys are checked at stage 4 against [Settings schema reference](/pages/reference-settings-schema/#common-row-keys).

| Key | Type and bounds | Default | Tier | Error code | Requirement |
|---|---|---|---|---|---|
| `support_url` | string, `https://` address, at most 256 characters | none | all | `manifest.schema`; warning `manifest.support-missing` when neither `support_url` nor `repository` is set | [PRP-004] |
| `imports_from` | list of at most 4 distinct plugin ids | none | all | `manifest.schema`; `manifest.id-conflict` when the list holds the plugin's own id; `import.owner-mismatch` at import time | [PRO-016] |
| `min_data_version` | integer, 1 to `data_version` | 1 | all | `manifest.data-version` | [Data migration](/pages/guide-data-migration/) |
| `accessibility` | table with `screen_reader`, `high_contrast`, `reduced_motion` and `compact_40_columns`, each `none`, `partial` or `full`, and `languages`, at most 64 BCP 47 tags | none (shown as `no accessibility declaration`) | all | `manifest.schema`; `check.accessibility-claim` from `ilium plugin check` when a fixture run does not meet a declared level | [PRX-008] |
| `contributes.scenes[].clip_seconds` | integer, 1 to 60 | 8 | all | `manifest.schema`; used only when `modes` holds `pre-rendered` | [Scenes](/pages/core-plugin-scenes/) |
| `limits.cpu_ms_per_s` | integer, 10 to 1,000 | 100 | T2 and T3; ignored with a `lint.limit-ignored` warning in a T1 plugin | `manifest.schema` | [SEC-029] |
| settings row `bound_hosts` | list of 1 to 16 host names, each at most 253 characters, with an optional leading `*.` label | none | all; only on a `text` row with `secret: true` | `manifest.settings-key` | [PRR-014] |
| settings row `redact` | `"auto"` or `"always"` | `"auto"` | all; only on a `text` row with `secret: true` | `manifest.settings-key` | [PRQ-014] |

- **[PRM-105]** The system must accept, in the published manifest schema and in the settings row checks, every manifest key and settings row key that a page of this manual specifies, and must state for each its type, bounds, default, tier restriction and error code on this page or on Settings schema reference. The schema test must validate every manifest example of the manual against the schema (a fragment that omits required keys is checked for unknown keys and for value types only) and must fail naming the page and the key on any key that the schema rejects. Before the check the schema test must replace each documentation placeholder with a valid sample value: a string value written wholly as `<text>` in angle brackets (for example `<64 hex digits>`) and each template token `@@name@@` of [Project templates](/pages/reference-project-templates/). The single exemption is an example on a page whose status line says `reserved`: there the test must assert the opposite, that the schema refuses the reserved name (for example the export `editor-provider`). A manifest example is every JSON code block whose top-level keys are all manifest keys and that has `api`, `contributes`, `capabilities`, `requires`, `activation`, `server`, `client` or `limits` among them. At the time of writing the manual has 177 such blocks with `api` and 77 without it, and the test must report both counts so that a drop is visible.

## Stages and exit codes

The numbered stages are those of the [validation order](/pages/reference-package-format/#the-validation-order). Letters mark codes raised outside the install pipeline.

| Stage | Name |
|---|---|
| 1 | archive |
| 2 | manifest-parse |
| 3 | schema |
| 4 | semantic |
| 5 | inventory |
| 6 | components |
| 7 | signature |
| 8 | trust |
| 9 | policy |
| 10 | dependencies |
| 11 | review |
| I | install transaction |
| R | registry client |
| C | command line |
| L | load |
| T | toolchain |
| H | host runtime: a log line, a notice, an audit record or the message of a call error |

Exit codes are those of [Distribution commands](/pages/reference-plugin-distribution-cli/#exit-codes). The column Exit below gives the exit code of the command that prints the code.

| Id | Rule |
|---|---|
| [PKG-571] | Every error is one JSONL line with `type` `error`, a `code` from this page, a `message` built from the template, a `stage` (number or letter as above) and, when a place exists, a `pointer` (RFC 6901) or a `path`. |
| [PKG-572] | A template placeholder in braces MUST be replaced with the value. A template MUST NOT contain a secret, a token or the content of a file outside the package. |
| [PKG-573] | The `message` is English text. The `code` is the stable interface: tools and tests match the code and never the message. |
| [PKG-574] | When several errors occur, the exit code is that of the first error printed. |
| [PKG-578] | Within a stage, errors are printed in the order of the central directory entry they concern (errors without an entry first), and for one entry in the order of the check list of the stage. Manifest errors sort by JSON pointer in byte order. A conformance row that names an expected result lists every code it expects, in print order, not only the first. |

## Error catalogue

This catalogue has 805 codes, of which 109 are in the `manifest.*` family. The two families `lint.*` and `harness.*` are counted on the pages that hold their rule tables. A code that appears in an older page and is not here is in the table of retired codes below.

| Id | Rule |
|---|---|
| [PKG-575] | A host MUST NOT print a code that is not in this catalogue. A new code needs an entry here first. |
| [PKG-576] | Every code of this catalogue MUST appear in at least one test of the conformance suite with the stage and the exit code shown. |

### Archive codes (`package.*`, stage 1)

Raised while the reader opens the `.iliumpkg` file. The reader collects up to 100 errors.

| Code | Stage | Exit | Message template |
|---|---|---|---|
| `package.zip-invalid` | 1 | 1 | `{path}: this file is not a valid ZIP archive ({reason}).` |
| `package.zip64` | 1 | 1 | `{path}: ZIP64 structures are not allowed.` |
| `package.zip-compressed` | 1 | 1 | `{entry}: the entry uses compression method {method}. Only stored entries are allowed.` |
| `package.zip-encrypted` | 1 | 1 | `{entry}: encrypted entries are not allowed.` |
| `package.zip-descriptor` | 1 | 1 | `{entry}: data descriptors are not allowed.` |
| `package.zip-extra` | 1 | 1 | `{entry}: extra fields are not allowed.` |
| `package.zip-comment` | 1 | 1 | `{entry}: archive and entry comments are not allowed.` |
| `package.zip-trailing` | 1 | 1 | `{path}: {count} bytes lie outside the ZIP record structure.` |
| `package.header-mismatch` | 1 | 1 | `{entry}: the local header does not match the central directory, or entries overlap.` |
| `package.crc-mismatch` | 1 | 1 | `{entry}: the CRC-32 is {found}, the header says {expected}.` |
| `package.path-invalid` | 1 | 1 | `{entry}: the path breaks rule {rule}.` |
| `package.path-duplicate` | 1 | 1 | `{entry}: the path appears twice.` |
| `package.path-case-collision` | 1 | 1 | `{entry}: the path differs only in letter case from {other}.` |
| `package.entry-directory` | 1 | 1 | `{entry}: directory entries are not allowed.` |
| `package.entry-symlink` | 1 | 1 | `{entry}: links are not allowed.` |
| `package.entry-order` | 1 | 1 | `{entry}: the entry is not in byte order.` |
| `package.entry-unexpected` | 1 | 1 | `{entry}: this location is not part of the package layout.` |
| `package.entry-limit` | 1 | 1 | `The archive holds {count} entries. The limit is 4096.` |
| `package.size-limit` | 1 | 1 | `{entry}: {size} bytes is over the limit of {limit} bytes.` |
| `package.manifest-size` | 1 | 1 | `manifest.json is {size} bytes. The limit is 1048576.` |
| `package.manifest-missing` | 1 | 1 | `The archive has no manifest.json.` |
| `package.old-format` | 1 | 1 | `{path} is an .iliumanim package. Port it with the steps in the porting guide.` |
| `package.precompiled-forbidden` | 1 | 1 | `{entry}: precompiled .cwasm files are not allowed.` |

### Manifest parse and schema codes (`manifest.*` and one `package.*` code, stages 2 and 3)

Stage 2 stops at its first error. Stage 3 collects all errors. Every message carries the JSON pointer of the value.

| Code | Stage | Exit | Message template |
|---|---|---|---|
| `manifest.json-invalid` | 2 | 1 | `manifest.json is not valid JSON or not UTF-8 ({reason}) at byte {offset}.` |
| `manifest.duplicate-key` | 2 | 1 | `The key "{key}" appears twice in the object at {pointer}.` |
| `manifest.schema` | 3 | 1 | `{pointer}: {reason}.` |
| `manifest.unknown-key` | 3 | 1 | `{pointer}: "{key}" is not a key of this API version.` |
| `manifest.key-missing` | 3 | 1 | `{pointer}: the required key "{key}" is missing.` |
| `manifest.id-invalid` | 3 | 1 | `The id "{id}" is not valid: {reason}.` |
| `manifest.id-reserved` | 3 | 1 | `The id "{id}" uses a reserved prefix. Only the Ilium release may use "ilium.".` |
| `manifest.version-invalid` | 3 | 1 | `The version "{version}" is not a valid semantic version.` |
| `manifest.display-name` | 3 | 1 | `{pointer}: the text contains a control, line or bidirectional character.` |
| `manifest.data-version` | 3 | 1 | `data_version must be an integer from 1 to 4294967295. Found {value}.` |
| `manifest.schema-keyword` | 3 | 1 | `{pointer}: the schema keyword "{keyword}" is not supported.` |
| `manifest.requires-invalid` | 3 | 1 | `{pointer}: "{value}" is not a valid {what}.` |
| `manifest.requires-self` | 3 | 1 | `The plugin {id} lists itself in requires.plugins.` |
| `manifest.when-syntax` | 3 | 1 | `{pointer}: the when expression is not valid at column {column}.` |
| `manifest.when-key` | 3 | 1 | `{pointer}: "{key}" is not a context key.` |
| `manifest.visible-when` | 3 | 1 | `{pointer}: visible_when {reason}.` |
| `manifest.contribution-limit` | 3 | 1 | `{pointer}: {count} entries. The limit for "{key}" is {limit}.` |
| `manifest.row-format` | 3 | 1 | `{pointer}: {reason}.` |
| `manifest.cli-field` | 3 | 1 | `{pointer}: "{value}" is not allowed for {field}.` |
| `manifest.menu-depth` | 3 | 1 | `{pointer}: menu items are nested {depth} levels deep. The limit is 2.` |
| `manifest.scene-prerender` | 3 | 1 | `{pointer}: {reason}.` |
| `manifest.settings-kind` | 3 | 1 | `{pointer}: "{value}" is not a settings entry kind.` |
| `manifest.settings-id` | 3 | 1 | `{pointer}: "{value}" is not a valid settings id.` |
| `manifest.settings-row-type` | 3 | 1 | `{pointer}: "{value}" is not a row type.` |
| `manifest.settings-row-field` | 3 | 1 | `{pointer}: the field "{field}" is not valid for a {type} row.` |
| `manifest.settings-default` | 3 | 1 | `{pointer}: the default does not match the row.` |
| `manifest.settings-limit` | 3 | 1 | `{pointer}: {count} {what}. The limit is {limit}.` |
| `manifest.settings-key` | 3 | 1 | `{pointer}: "{value}" is not a valid settings key.` |
| `manifest.settings-help` | 3 | 1 | `{pointer}: the help topic "{topic}" is not listed under contributes.help.` |
| `manifest.settings-command` | 3 | 1 | `{pointer}: the command "{command}" is not declared by this plugin.` |
| `manifest.settings-glyph` | 3 | 1 | `{pointer}: "{value}" must be one character 1 or 2 cells wide.` |
| `manifest.settings-source` | 3 | 1 | `{pointer}: a row needs exactly one of options and source.` |
| `manifest.sound-id-taken` | 3 | 1 | `{pointer}: the sound id "{id}" is used twice or is reserved.` |
| `manifest.sound-format` | 3 | 1 | `{pointer}: the sound file "{path}" is missing or is not WAV PCM or OGG Vorbis.` |
| `package.api-unsupported` | 3 | 1 | `The api {api} is outside the window of this Ilium ({window}).` |

### Semantic codes (`manifest.*`, stage 4)

Cross references, uniqueness and side rules. Stage 4 collects all errors.

| Code | Stage | Exit | Message template |
|---|---|---|---|
| `manifest.entry-invalid` | 4 | 1 | `{pointer}: the entry "{entry}" must be "{expected}".` |
| `manifest.grant-unknown` | 4 | 1 | `{pointer}: "{grant}" is not a capability of ilium:plugin@0.1.0.` |
| `manifest.grant-argument` | 4 | 1 | `{pointer}: the argument of "{grant}" is not valid: {reason}.` |
| `manifest.grant-argument-missing` | 4 | 1 | `{pointer}: "{grant}" needs an argument.` |
| `manifest.grant-wildcard` | 4 | 1 | `{pointer}: "{grant}" does not allow a wildcard.` |
| `manifest.grant-side` | 4 | 1 | `{pointer}: "{grant}" is not available on the {side} side.` |
| `manifest.grant-dependency` | 4 | 1 | `{pointer}: "{grant}" needs "{needed}" on the same side.` |
| `manifest.grant-scope` | 4 | 1 | `{pointer}: the scope "{scope}" is unknown or does not accept {access}.` |
| `manifest.grant-program` | 4 | 1 | `{pointer}: "{value}" must be a program name or an absolute path.` |
| `manifest.grant-missing-extension` | 4 | 1 | `{pointer}: the extension entry needs the grant "{grant}".` |
| `manifest.settings-id-clash` | 4 | 1 | `{pointer}: the settings id "{id}" is used twice.` |
| `manifest.settings-title-clash` | 4 | 1 | `{pointer}: the tab title "{title}" is used twice.` |
| `manifest.settings-key-clash` | 4 | 1 | `{pointer}: the settings key "{key}" is used twice.` |
| `manifest.settings-target-unknown` | 4 | 1 | `{pointer}: "{tab}" is not a settings tab.` |
| `manifest.settings-target-closed` | 4 | 1 | `{pointer}: the tab "{tab}" does not accept sections.` |
| `manifest.settings-target-grant` | 4 | 1 | `{pointer}: a section for "{tab}" needs the grant "{grant}".` |
| `manifest.settings-anchor` | 4 | 1 | `{pointer}: the anchor "{anchor}" is not a known row or section.` |
| `manifest.settings-tab-anchor` | 4 | 1 | `{pointer}: the tab anchor "{anchor}" is not a known tab.` |
| `manifest.settings-depends` | 4 | 1 | `{pointer}: depends_on names the unknown key "{key}" or forms a cycle.` |
| `manifest.settings-owner` | 4 | 1 | `{pointer}: another plugin already owns the settings tab "{tab}".` |
| `manifest.cli-name-reserved` | 4 | 1 | `{pointer}: the command line name "{name}" is a kernel subcommand.` |
| `manifest.cli-name-taken` | 4 | 1 | `{pointer}: the command line name "{name}" is already used.` |
| `manifest.contribution-target` | 4 | 1 | `{pointer}: "{target}" is not a valid target.` |
| `manifest.catalog-unknown` | 4 | 1 | `{pointer}: the catalogue "{catalog}" does not exist.` |
| `manifest.client-request-unknown` | 4 | 1 | `{pointer}: the client request "{kind}" does not match the other side.` |
| `manifest.extension-point-duplicate` | 4 | 1 | `{pointer}: the extension point "{id}" is declared twice.` |
| `manifest.extension-point-invalid` | 4 | 1 | `{pointer}: {reason}.` |
| `manifest.extension-entry-invalid` | 4 | 1 | `{pointer}: the entry does not match the schema of {point}: {reason}.` |
| `manifest.extension-side` | 4 | 1 | `{pointer}: the handler must be on the {side} side.` |
| `manifest.extension-limit` | 4 | 1 | `{pointer}: {count} entries. The limit is {limit}.` |
| `manifest.unstable-unknown` | 4 | 1 | `{pointer}: "{name}" is not an unstable feature.` |

### Inventory and component codes (stages 5 and 6)

Stage 5 compares `files` with the archive. Stage 6 reads the components without running them.

| Code | Stage | Exit | Message template |
|---|---|---|---|
| `manifest.files-order` | 5 | 1 | `files is not in archive order at {pointer}.` |
| `manifest.file-self` | 5 | 1 | `files must not list {path}.` |
| `manifest.file-unlisted` | 5 | 1 | `{path} is in the package but not in files.` |
| `manifest.file-missing` | 5 | 1 | `{path} is in files but not in the package.` |
| `manifest.file-length` | 5 | 1 | `{path}: {found} bytes, files says {expected}.` |
| `manifest.file-hash` | 5 | 1 | `{path}: the SHA-256 is {found}, files says {expected}.` |
| `manifest.asset-size` | 5 | 1 | `The assets total {size} bytes. The limit is {limit} bytes.` |
| `package.component-invalid` | 6 | 1 | `{path} is not a valid WebAssembly component for the world {world}.` |
| `package.api-section-missing` | 6 | 1 | `{path} has no ilium:api-version section.` |
| `package.api-section-repeated` | 6 | 1 | `{path} has more than one ilium:api-version section.` |
| `package.api-section-invalid` | 6 | 1 | `{path}: the ilium:api-version payload is {size} bytes. It must be 6.` |
| `manifest.api-version` | 6 | 1 | `{path} was built for {found}. The manifest says {expected}.` |
| `manifest.exports-mismatch` | 6 | 1 | `{path}: exports differ from the manifest. Missing: {missing}. Extra: {extra}.` |
| `manifest.export-grant` | 6 | 1 | `{path} exports {export}, which needs the grant {grant}.` |
| `manifest.route-grant` | 6 | 1 | `{path}: route {route} and grant {grant} do not match.` |
| `manifest.import-not-requested` | 6 | 1 | `{path} imports {interface}, which no requested capability unlocks.` |
| `manifest.unstable-missing` | 6 | 1 | `{path} imports {item}, which the unstable list does not name.` |

### Signature and trust codes (stages 7 and 8)

Signature checks stop at the first error. Trust checks assign the tier. The Exit column is the exit code of an install. `ilium plugin verify` prints `trust.key-unknown` as a `warning` line and keeps the exit code 0 ([PKG-156](/pages/reference-package-format/#offline-verification-ilium-plugin-verify)).

| Code | Stage | Exit | Message template |
|---|---|---|---|
| `package.signature-format` | 7 | 1 | `The signature entry is not a valid ilium-sig/1 document: {reason}.` |
| `manifest.signature-invalid` | 7 | 1 | `The signature of {id} does not match manifest.json.` |
| `trust.declined` | 8 | 1 | `Key {key_id} was not trusted. Nothing was installed.` |
| `trust.key-unknown` | 8 | 1 | `{id} is signed by the key {key_id}, which you do not trust.` |
| `trust.key-revoked` | 8 | 1 | `The key {key_id} was revoked on {date}: {reason}.` |
| `trust.bundled-tampered` | 8 | 1 | `The bundled plugin {id} does not match this Ilium release.` |
| `manifest.id-publisher` | 8 | 1 | `The id {id} must start with {publisher_id} because the package is signed for that publisher.` |
| `manifest.publisher-mismatch` | 8 | 1 | `The package says publisher "{manifest_publisher}". The key belongs to "{key_publisher}".` |
| `package.publisher-key-changed` | 8 | 1 | `{id} was installed with key {old_key_id}. This package uses {new_key_id}.` |
| `trust.key-malformed` | C | 1 | `{path} is not a valid public key file: {reason}.` |
| `trust.key-exists` | C | 1 | `{path} already exists. keygen does not overwrite keys.` |
| `trust.already-trusted` | C | 1 | `The key {key_id} is already trusted.` |
| `trust.not-found` | C | 5 | `No trusted key {key_id}.` |
| `trust.in-use` | C | 1 | `The key {key_id} signed {count} installed plugins: {ids}. Use --force to continue.` |
| `trust.system-entry` | C | 1 | `The key {key_id} comes from the system trust file. You cannot remove it.` |
| `trust.store-invalid` | C | 1 | `The trust store {path} is not valid: {reason}. It is treated as empty.` |
| `trust.publisher-conflict` | C | 1 | `The key {key_id} is already bound to {publisher_id}.` |
| `trust.publisher-reserved` | C | 1 | `The publisher id {publisher_id} is reserved.` |
| `trust.rotation-invalid` | C | 1 | `The rotation statement is not signed by a trusted key of {publisher_id}.` |
| `trust.collision` | C | 1 | `The short fingerprint {short} matches the trusted key {key_id}. Use the full fingerprint.` |
| `sign.key-permissions` | C | 1 | `{path} can be read by other users. Run: chmod 600 {path}` |
| `sign.key-unreadable` | C | 1 | `{path} is not a usable private key: {reason}.` |
| `sign.publisher-mismatch` | C | 1 | `The manifest publisher is "{manifest_publisher}". The key belongs to "{key_publisher}".` |
| `sign.already-signed` | C | 1 | `{path} already has a signature. Use --replace to sign again.` |

### Policy codes (stage 9)

Tier-dependent rules need the tier, so they run after trust. Administrator policy is in the [plugin policy](/pages/reference-plugin-policy/).

| Code | Stage | Exit | Message template |
|---|---|---|---|
| `manifest.grant-tier` | 9 | 1 | `{pointer}: "{grant}" is only available to bundled plugins.` |
| `manifest.limits-tier` | 9 | 1 | `{pointer}: limits_tier "first-party" is only for bundled ilium.* plugins.` |
| `manifest.unstable-tier` | 9 | 1 | `{pointer}: the unstable feature "{name}" needs development mode for a {tier} package.` |
| `manifest.replaces-tier` | 9 | 1 | `{pointer}: only bundled or trusted plugins can replace "{id}".` |
| `manifest.secret-scope` | 9 | 1 | `{pointer}: {reason}.` |
| `package.platform-unsupported` | 9 | 1 | `This package needs {needed}. This machine has {found}.` |
| `package.downgrade` | 9 | 1 | `{id} {installed} is installed. {version} is lower. Use --allow-downgrade to continue.` |
| `package.version-reused` | 9 | 1 | `{id} {version} is already installed with different content.` |
| `policy.blocked` | 9 | 7 | `The administrator policy blocks {id}: {detail}.` |
| `policy.third-party-denied` | 9 | 7 | `Third-party plugins are not allowed on this machine.` |
| `policy.tier-denied` | 9 | 7 | `Tier {tier} is not allowed on this machine.` |
| `policy.publisher-denied` | 9 | 7 | `The key {key_id} is not on the allowed publisher list.` |
| `policy.id-denied` | 9 | 7 | `The id {id} is denied, or not on the allowed list.` |
| `policy.grant-forbidden` | not an install error | 7 | `The grant {grant} is forbidden on this machine.` Shown next to the locked row; the install succeeds |
| `policy.source-denied` | 9 | 7 | `The package source {source} is not in the administrator list allowed_sources.` |
| `policy.replacement` | 9 | 7 | `The policy does not allow {id} to replace {target}.` |
| `policy.invalid` | C | 7 | `The policy file {path} is not valid at line {line}: {reason}.` |
| `policy.managed-plugin` | C | 7 | `{id} is managed by the administrator and cannot be changed here.` |
| `package.blocked` | 9 | 7 | `{id} {version} is on the block list of {source}: {reason}.` |

### Dependency codes (stage 10)

See [Versions and dependencies](/pages/reference-dependency-resolution/).

| Code | Stage | Exit | Message template |
|---|---|---|---|
| `dependency.unresolved` | 10 | 1 | `{id} needs {requirement}. No available version matches.` |
| `dependency.conflict` | 10 | 1 | `{id} {version} would leave {dependant} outside its range {range}.` |
| `dependency.cycle` | 10 | 1 | `The plugins {ids} require each other.` |
| `dependency.host-version` | 10 | 1 | `{id} needs Ilium {range}. This is Ilium {host}.` |
| `dependency.in-use` | 10 | 1 | `{id} is required by {dependants}.` |
| `dependency.lock-mismatch` | 10 | 1 | `The lock file says {expected}. The resolution gives {found}.` |
| `dependency.unsatisfied` | 10 | 1 | `{id} needs {dependency}, which is not in the set.` |
| `manifest.service-name` | 4 | 1 | `{pointer}: "{name}" is not a service name of this plugin.` |
| `manifest.service-version` | 4 | 1 | `{pointer}: "{value}" is not <name>@<major>.<minor>.<patch>.` |
| `manifest.service-cycle` | 10 | 1 | `The services {services} form a cycle.` |
| `manifest.replaces-unknown` | 10 | 1 | `{pointer}: "{id}" is not a core plugin.` |
| `manifest.replaces-not-replaceable` | 10 | 1 | `{pointer}: "{id}" cannot be replaced.` |
| `manifest.point-risk` | 10 | 1 | `{pointer}: the extension point declares a risk level lower than the grant its handlers receive, or an unknown risk.` |
| `manifest.replaces-surface-incomplete` | 10 | 1 | `{pointer}: the replacement does not provide {missing}.` |
| `manifest.replaces-cycle` | 10 | 1 | `{ids} replace each other.` |

### Install transaction codes

See [Install and update](/pages/reference-install-and-update/).

| Code | Stage | Exit | Message template |
|---|---|---|---|
| `install.locked` | I | 4 | `Another install holds the lock (process {pid}). Waited 30 s.` |
| `install.state-corrupt` | I | 1 | `installed.json cannot be read: {reason}.` |
| `install.registry-format` | I | 1 | `installed.json has format {found}. This Ilium knows up to {known}.` |
| `install.io` | I | 1 | `{operation} failed on {path}: {reason}.` |
| `install.no-space` | I | 1 | `Not enough free space on {volume}: {free} free, {needed} needed.` |
| `install.write-failed` | I | 1 | `{path} could not be written: {reason}.` |
| `install.notify-failed` | I | 1 | `The install is complete. Running servers could not be told ({count}).` |
| `install.not-installed` | C | 5 | `{id} is not installed.` |
| `install.no-previous` | C | 5 | `{id} has no earlier version on disk.` |
| `rollback.no-backup` | I | 1 | `The pre-update snapshot of {id} is gone.` |
| `migration.data-newer` | I | 1 | `{id} {version} supports data version {supported}. The stored data is at {stamp}. Use --restore-backup or --discard-data.` |
| `migration.interrupted` | I | 1 | `The migration of {id} was interrupted {attempts} times. The update was rolled back.` |
| `migration.failed` | I | 1 | `The migration of {id} failed: {reason}. The update was rolled back.` |
| `migration.backup-too-large` | I | 1 | `The data of {id} is {size}, over the backup limit of {limit}. Pass --no-backup.` |
| `migration.io` | I | 1 | `The data directory of {id} cannot be written: {reason}. The update was rolled back.` |
| `migration.skipped` | - | - | Log line only, not a failure: `{id} {version} has no migrator. The data version was stamped as {stamp}.` |
| `install.bundled` | C | 1 | `{id} is part of Ilium and cannot be changed.` |
| `install.scope-unsupported` | C | 1 | `Only user scope installs are available.` |
| `install.unsigned` | 8 | 1 | `{id} is unsigned. Pass --allow-unsigned to install it.` |
| `install.review-required` | 11 | 1 | `No person can review the grants. Pass --yes.` |
| `install.review-declined` | 11 | 1 | `The review was declined.` |
| `install.needs-input` | 11 | 1 | `A decision is needed and no person can answer. {what}` |
| `install.cancelled` | 11, or any step before the commit | 1 | `The review was closed.` when the review dialog was closed; `Install cancelled. Nothing was changed.` when the user cancelled a running install or update ([PRM-204](/pages/user-install-and-manage/#install-from-a-file)) |
| `install.compile-failed` | I | 1 | `{path} did not compile on this machine: {reason}.` |
| `install.source-scheme` | C | 1 | `The scheme "{scheme}" is not supported. Use https or a file path.` |
| `install.insecure-url` | C | 1 | `{url}: only https addresses are allowed.` |
| `install.download-failed` | R | 8 | `{url}: the download failed ({reason}).` |
| `install.size` | R | 1 | `{url}: the download is over 300 MiB.` |
| `package.corrupt` | L | 1 | `The stored copy of {id} {version} fails its hash check.` |
| `package.cache-missing` | L | 1 | `The cached package of {id} {version} is gone.` |
| `plugin.not-found` | C | 5 | `No plugin with the id {id}.` |
| `plugin.not-running` | C | 1 | `{id} is not running.` |
| `plugin.not-removable` | C | 1 | `{id} cannot be removed.` |
| `plugin.already-installed` | C | 1 | `{id} is already installed.` |
| `plugin.server-missing` | C | 1 | `No server is running for this project.` |
| `plugin.version-skew` | C | 1 | `The server runs {server}. This client runs {client}.` |
| `grants.not-requested` | C | 1 | `{id} does not request the grant {grant}.` |

### Result codes

These codes are printed on lines of type `result` or `warning`, never `error`. Only lines of `type` `error` decide the exit code of a command; a result or warning line leaves it at 0.

| Code | Type | Stage | Exit | Message template |
|---|---|---|---|---|
| `install.already-installed` | warning | I | 0 | `{id} {version} is already installed with the same content.` |
| `update.ok` | result | I | 0 | `{id} {version} is active.` |
| `update.rolled-back` | warning | I | 0 | `{id} {candidate} failed to activate. {id} {version} is active again.` |
| `uninstall.ok` | result | C | 0 | `{id} was uninstalled.` |
| `uninstall.purged` | result | C | 0 | `Removed {item}.` |
| `uninstall.recovered` | warning | I | 0 | `An interrupted uninstall of {id} was completed.` |

### Registry client codes

See [Registry reference](/pages/reference-registry/).

| Code | Stage | Exit | Message template |
|---|---|---|---|
| `registry.none-configured` | R | 1 | `No registry is configured. Add one with: ilium plugin registry add` |
| `registry.root-key-missing` | R | 2 | `A registry needs --root-key.` |
| `registry.insecure-url` | R | 1 | `{url}: only https registries are allowed.` |
| `registry.unreachable` | R | 8 | `{url} cannot be reached ({reason}).` |
| `registry.metadata-expired` | R | 8 | `The {document} document of {url} expired on {date}.` |
| `registry.signature-invalid` | R | 1 | `The {document} document of {url} has an invalid signature.` |
| `registry.rollback` | R | 1 | `The {document} version {found} is lower than the version {known} seen before.` |
| `registry.hash-mismatch` | R | 1 | `The package {id} {version} does not match the hash in the registry.` |
| `registry.not-found` | R | 5 | `{id} is not in the registry.` |
| `registry.yanked` | R | 1 | `{id} {version} was withdrawn: {reason}.` |
| `registry.size` | R | 1 | `{document} is over its size limit of {limit} bytes.` |
| `registry.changed` | R | 1 | `The root key of {url} changed. Remove and add the registry again if you trust the new key.` |
| `registry.auth` | R | 1 | `The registry rejected the credential.` |
| `registry.policy` | R | 7 | `The policy does not allow the registry {url}.` |
| `registry.takedown` | R | 1 | `{id} {version} was taken down by {url}: {reason}.` |
| `registry.version-exists` | R | 1 | `{id} {version} is already published.` |
| `registry.version-invalid` | R | 1 | `The version {version} cannot be published: {reason}.` |
| `registry.id-reserved` | R | 1 | `The id {id} is reserved.` |
| `registry.namespace-unowned` | R | 1 | `You do not own the namespace {namespace}.` |
| `registry.confusable` | R | 1 | `The id {id} looks like {other}, which belongs to another publisher.` |
| `registry.category-unknown` | R | 1 | `The category "{category}" is not in the list.` |
| `publish.unsigned` | C | 1 | `{path} is not signed. Sign it first.` |

### Usage, tool and session codes

Raised by commands before any work starts.

| Code | Stage | Exit | Message template |
|---|---|---|---|
| `usage.missing-argument` | C | 2 | `The argument {name} is required.` |
| `usage.unknown-flag` | C | 2 | `The flag {flag} is not known.` |
| `usage.invalid-value` | C | 2 | `The value "{value}" is not valid for {name}: {reason}.` |
| `usage.conflict` | C | 2 | `The flags {first} and {second} cannot be used together.` |
| `usage.file-not-found` | C | 2 | `{path}: no such file.` |
| `usage.subject-unavailable` | C | 2 | `{subject} is not available here.` |
| `usage.release-build-required` | C | 2 | `This command needs a release build.` |
| `session.unreachable` | C | 6 | `No session answers at {path}.` |
| `session.not-found` | C | 6 | `No session named {name}.` |
| `session.version-mismatch` | C | 1 | `The session runs {server}. This command is {client}.` |
| `session.memory-exceeded` | C | 1 | `The session uses more memory than its limit of {limit}.` |
| `toolchain.bun-missing` | T | 4 | `Bun is not installed.` |
| `toolchain.bun-version` | T | 4 | `Bun {found} is installed. {needed} is needed.` |
| `toolchain.rustc-missing` | T | 4 | `rustc is not installed.` |
| `toolchain.rust-version` | T | 4 | `rustc {found} is installed. {needed} is needed.` |
| `toolchain.rust-target` | T | 4 | `The target {target} is not installed.` |
| `toolchain.sdk-missing` | T | 4 | `The SDK {sdk} is not installed.` |
| `toolchain.sdk-mismatch` | T | 4 | `The SDK is {found}. The manifest api says {expected}.` |
| `toolchain.language-ambiguous` | T | 4 | `Both Cargo.toml and package.json exist. Pass --language.` |
| `toolchain.language-unknown` | T | 4 | `No supported language found in {path}.` |
| `toolchain.lockfile-missing` | T | 4 | `No lock file in {path}.` |
| `toolchain.network-needed` | T | 4 | `The build needs the network and --offline was given.` |
| `toolchain.coverage-tool-missing` | T | 4 | `The coverage tool {tool} is not installed.` |

### Codes added by later requirements

The tables below hold every code that another page of this manual names and that the sections above do not hold. The column Defined on names the page that states the cause of the code and what the user does next; `ilium plugin explain <code>` prints both sentences from that page. A code of stage H is written to the plugin log, a notice or an audit record, or is carried in the message of a call error. It is printed by an `ilium plugin` command only when the Exit column holds a number; `-` means that no command exits with it. An exit code of 0 marks a `warning` or `result` line. Three rules complete the Exit column:

- A finding of `ilium plugin check` or `ilium plugin lint` outside the `lint.*` family (for example `a11y.sound-only`, `i18n.untranslated` or `ui.flash-risk`) has exit code 3 when its page gives it the severity `error` and 0 when it gives `warn` or `info`. A finding whose page states no severity is a warning. Under `--deny warnings` every finding has exit code 3.
- `size.component-over-budget` is a warning above 2 MiB and an error, with exit code 3, above the pack limit.
- `ipc.protocol-mismatch` is the one code whose exit code, 76, is that of the `ilium` client and not of an `ilium plugin` command.

#### Semantic codes (stage 4)

| Code | Stage | Exit | Defined on |
|---|---|---|---|
| `manifest.component-too-large` | 4 | 1 | [reference-sandbox](/pages/reference-sandbox/) |
| `manifest.deprecated-expired` | 4 | 1 | [guide-deprecating-plugin-features](/pages/guide-deprecating-plugin-features/) |
| `manifest.deprecated-invalid` | 4 | 1 | [guide-deprecating-plugin-features](/pages/guide-deprecating-plugin-features/) |
| `manifest.description-missing` | 4 | 1 | [a11y-non-text-panes](/pages/a11y-non-text-panes/) |
| `manifest.i18n-reference-invalid` | 4 | 1 | [guide-internationalisation](/pages/guide-internationalisation/) |
| `manifest.id-conflict` | 4 | 1 | [reference-manifest-schema](/pages/reference-manifest-schema/) |
| `manifest.keybinding-conflict` | 4 | 1 | [core-plugin-keys-and-help](/pages/core-plugin-keys-and-help/) |
| `manifest.keybinding-reserved` | 4 | 1 | [core-plugin-keys-and-help](/pages/core-plugin-keys-and-help/) |
| `manifest.limit` | 4 | 1 | [reference-manifest](/pages/reference-manifest/) |
| `manifest.listing-invalid` | 4 | 1 | [reference-manifest](/pages/reference-manifest/) |
| `manifest.sound-length` | 4 | 1 | [capability-sound-play](/pages/capability-sound-play/) |
| `manifest.support-missing` | 4 | 0 | [reference-manifest](/pages/reference-manifest/) |
| `manifest.telemetry-host-ungranted` | 4 | 1 | [reference-manifest](/pages/reference-manifest/) |
| `manifest.view-help` | 4 | 1 | [core-plugin-keys-and-help](/pages/core-plugin-keys-and-help/) |
| `manifest.wit-mismatch` | 4 | 1 | [reference-abi-evolution](/pages/reference-abi-evolution/) |

#### Inventory codes (stage 5)

| Code | Stage | Exit | Defined on |
|---|---|---|---|
| `package.build-mismatch` | 5 | 1 | [reference-package-format](/pages/reference-package-format/) |
| `package.changed-after-review` | 5 | 1 | [reference-install-and-update](/pages/reference-install-and-update/) |
| `package.path-collision` | 5 | 1 | [reference-plugin-files](/pages/reference-plugin-files/) |
| `package.path-too-long` | 5 | 1 | [reference-plugin-files](/pages/reference-plugin-files/) |
| `package.provenance-mismatch` | 5 | 1 | [reference-trust-and-supply-chain](/pages/reference-trust-and-supply-chain/) |
| `package.provenance-missing` | 5 | 1 | [reference-trust-and-supply-chain](/pages/reference-trust-and-supply-chain/) |
| `package.sdk-vulnerable` | 5 | 1 | [reference-trust-and-supply-chain](/pages/reference-trust-and-supply-chain/) |
| `package.too-large` | 5 | 1 | [reference-install-and-update](/pages/reference-install-and-update/) |

#### Component codes (stage 6)

| Code | Stage | Exit | Defined on |
|---|---|---|---|
| `component.custom-section-too-large` | 6 | 1 | [reference-errors-and-limits](/pages/reference-errors-and-limits/) |
| `component.import-unknown` | 6 | 1 | [reference-errors-and-limits](/pages/reference-errors-and-limits/) |
| `component.size-warning` | 6 | 0 | [reference-check-and-lint-rules](/pages/reference-check-and-lint-rules/) |
| `component.too-large` | 6 | 1 | [reference-errors-and-limits](/pages/reference-errors-and-limits/) |
| `component.wasi-forbidden` | 6 | 1 | [reference-host-runtime](/pages/reference-host-runtime/) |
| `component.world-mismatch` | 6 | 1 | [reference-check-and-lint-rules](/pages/reference-check-and-lint-rules/) |

#### Trust codes (stage 8)

| Code | Stage | Exit | Defined on |
|---|---|---|---|
| `provenance.refused` | 8 | 1 | [reference-trust-and-supply-chain](/pages/reference-trust-and-supply-chain/) |
| `trust.clock-skew` | 8 | 1 | [reference-signing-and-trust](/pages/reference-signing-and-trust/) |
| `trust.log-inconsistent` | 8 | 1 | [reference-registry](/pages/reference-registry/) |
| `trust.log-split-view` | 8 | 1 | [reference-trust-and-supply-chain](/pages/reference-trust-and-supply-chain/) |
| `trust.removed` | 8 | 1 | [reference-plugin-lifecycle-states](/pages/reference-plugin-lifecycle-states/) |
| `trust.revocation-invalid` | 8 | 1 | [reference-signing-and-trust](/pages/reference-signing-and-trust/) |
| `trust.revocation-stale` | 8 | 8 | [reference-admin-policy](/pages/reference-admin-policy/) |
| `trust.signature-expired` | 8 | 1 | [reference-signing-and-trust](/pages/reference-signing-and-trust/) |
| `trust.signature-not-yet-valid` | 8 | 1 | [reference-signing-and-trust](/pages/reference-signing-and-trust/) |
| `trust.source-changed` | 8 | 1 | [reference-trust-and-supply-chain](/pages/reference-trust-and-supply-chain/) |
| `trust.store-tampered` | 8 | 1 | [reference-plugin-files](/pages/reference-plugin-files/) |

#### Policy codes (stage 9)

| Code | Stage | Exit | Defined on |
|---|---|---|---|
| `policy.locked` | 9 | 7 | [reference-admin-policy](/pages/reference-admin-policy/) |
| `policy.publisher-unverified` | 9 | 7 | [reference-admin-policy](/pages/reference-admin-policy/) |
| `policy.review-level-too-low` | 9 | 7 | [reference-admin-policy](/pages/reference-admin-policy/) |
| `policy.user-quota` | 9 | 7 | [guide-enterprise-deployment](/pages/guide-enterprise-deployment/) |
| `policy.version-denied` | 9 | 7 | [reference-admin-policy](/pages/reference-admin-policy/) |
| `policy.version-pinned` | 9 | 7 | [reference-admin-policy](/pages/reference-admin-policy/) |

#### Dependency codes (stage 10)

| Code | Stage | Exit | Defined on |
|---|---|---|---|
| `dependency.lock-source-refused` | 10 | 2 | [reference-dependency-resolution](/pages/reference-dependency-resolution/) |
| `dependency.revoked` | 10 | 1 | [reference-dependency-resolution](/pages/reference-dependency-resolution/) |
| `dependency.source-mismatch` | 10 | 1 | [reference-dependency-resolution](/pages/reference-dependency-resolution/) |
| `resolve.graph-too-large` | 10 | 1 | [reference-dependency-resolution](/pages/reference-dependency-resolution/) |

#### Install, update and data movement codes (stage I)

| Code | Stage | Exit | Defined on |
|---|---|---|---|
| `backup.inside-live-root` | I | 1 | [reference-plugin-files](/pages/reference-plugin-files/) |
| `backup.no-space` | I | 1 | [reference-offline-and-low-disk](/pages/reference-offline-and-low-disk/) |
| `backup.not-cross-bucket-consistent` | I | 1 | [reference-plugin-files](/pages/reference-plugin-files/) |
| `backup.project-missing` | I | 1 | [reference-plugin-scope-and-sessions](/pages/reference-plugin-scope-and-sessions/) |
| `bundle.exists` | I | 1 | [core-plugin-diagnostics](/pages/core-plugin-diagnostics/) |
| `bundle.plugin-unknown` | I | 5 | [getting-started-first-plugin](/pages/getting-started-first-plugin/) |
| `bundle.root-not-pinned` | I | 1 | [guide-air-gapped-transfer](/pages/guide-air-gapped-transfer/) |
| `bundle.scope-conflict` | I | 1 | [core-plugin-diagnostics](/pages/core-plugin-diagnostics/) |
| `bundle.scope-missing` | I | 1 | [core-plugin-diagnostics](/pages/core-plugin-diagnostics/) |
| `bundle.timeout` | I | 1 | [core-plugin-diagnostics](/pages/core-plugin-diagnostics/) |
| `bundle.too-large` | I | 1 | [core-plugin-diagnostics](/pages/core-plugin-diagnostics/) |
| `bundle.write-failed` | I | 1 | [core-plugin-diagnostics](/pages/core-plugin-diagnostics/) |
| `erase.incomplete` | I | 3 | [user-privacy](/pages/user-privacy/) |
| `export.exists` | I | 1 | [reference-install-update-uninstall](/pages/reference-install-update-uninstall/) |
| `export.inside-live-root` | I | 1 | [user-privacy](/pages/user-privacy/) |
| `export.target-not-empty` | I | 1 | [user-privacy](/pages/user-privacy/) |
| `export.unknown-plugin` | I | 5 | [reference-install-update-uninstall](/pages/reference-install-update-uninstall/) |
| `import.format-unsupported` | I | 1 | [reference-plugin-files](/pages/reference-plugin-files/) |
| `import.from-id-mismatch` | I | 1 | [guide-data-migration](/pages/guide-data-migration/) |
| `import.id-mismatch` | I | 1 | [guide-data-migration](/pages/guide-data-migration/) |
| `import.owner-mismatch` | I | 1 | [reference-plugin-lifecycle-states](/pages/reference-plugin-lifecycle-states/) |
| `install.busy` | I | 4 | [reference-install-update-uninstall](/pages/reference-install-update-uninstall/) |
| `install.dev-attached` | I | 4 | [reference-install-update-uninstall](/pages/reference-install-update-uninstall/) |
| `install.file-busy` | I | 4 | [reference-install-update-uninstall](/pages/reference-install-update-uninstall/) |
| `install.kept-data-owner-changed` | I | 1 | [reference-install-update-uninstall](/pages/reference-install-update-uninstall/) |
| `install.path-too-long` | I | 1 | [reference-plugin-files](/pages/reference-plugin-files/) |
| `install.readonly` | I | 1 | [reference-offline-and-low-disk](/pages/reference-offline-and-low-disk/) |
| `install.registry-conflict` | I | 4 | [reference-plugin-files](/pages/reference-plugin-files/) |
| `install.store-too-large` | I | 1 | [reference-install-update-uninstall](/pages/reference-install-update-uninstall/) |
| `install.system-requires-admin` | I | 1 | [guide-enterprise-deployment](/pages/guide-enterprise-deployment/) |
| `install.too-many` | I | 1 | [reference-install-update-uninstall](/pages/reference-install-update-uninstall/) |
| `install.uninstalling` | I | 4 | [reference-plugin-lifecycle-states](/pages/reference-plugin-lifecycle-states/) |
| `install.unsupported-platform` | I | 1 | [reference-minimum-system-requirements](/pages/reference-minimum-system-requirements/) |
| `install.update-busy` | I | 4 | [reference-install-update-uninstall](/pages/reference-install-update-uninstall/) |
| `lock.invalid` | I | 1 | [reference-dependency-resolution](/pages/reference-dependency-resolution/) |
| `lock.mismatch` | I | 1 | [guide-enterprise-deployment](/pages/guide-enterprise-deployment/) |
| `lock.source-unreachable` | I | 8 | [guide-enterprise-deployment](/pages/guide-enterprise-deployment/) |
| `migrate.failed` | I | 1 | [guide-data-migration](/pages/guide-data-migration/) |
| `migration.cancelled` | I | 1 | [guide-data-migration](/pages/guide-data-migration/) |
| `migration.no-backup-failed` | I | 1 | [guide-data-migration](/pages/guide-data-migration/) |
| `migration.too-old` | I | 1 | [guide-data-migration](/pages/guide-data-migration/) |
| `project.missing` | I | 1 | [reference-plugin-scope-and-sessions](/pages/reference-plugin-scope-and-sessions/) |
| `project.owned-by-other-user` | I | 1 | [guide-enterprise-deployment](/pages/guide-enterprise-deployment/) |
| `project.trust-orphan` | I | 1 | [reference-plugin-scope-and-sessions](/pages/reference-plugin-scope-and-sessions/) |
| `purge.applied` | I | 0 | [reference-plugin-files](/pages/reference-plugin-files/) |
| `purge.server-running` | I | 1 | [user-privacy](/pages/user-privacy/) |
| `purge.user-active` | I | 1 | [guide-enterprise-deployment](/pages/guide-enterprise-deployment/) |
| `restore.dropped-data-version` | I | 1 | [guide-data-migration](/pages/guide-data-migration/) |
| `restore.duplicate-entry` | I | 1 | [guide-backup-and-restore](/pages/guide-backup-and-restore/) |
| `restore.expansion-limit` | I | 1 | [guide-backup-and-restore](/pages/guide-backup-and-restore/) |
| `restore.format-unsupported` | I | 1 | [reference-plugin-files](/pages/reference-plugin-files/) |
| `restore.host-newer` | I | 1 | [reference-plugin-files](/pages/reference-plugin-files/) |
| `restore.host-too-old` | I | 1 | [guide-backup-and-restore](/pages/guide-backup-and-restore/) |
| `restore.no-space` | I | 1 | [reference-offline-and-low-disk](/pages/reference-offline-and-low-disk/) |
| `restore.plugin-not-installed` | I | 5 | [reference-plugin-files](/pages/reference-plugin-files/) |
| `restore.server-running` | I | 1 | [guide-backup-and-restore](/pages/guide-backup-and-restore/) |
| `restore.unsafe-entry` | I | 1 | [guide-backup-and-restore](/pages/guide-backup-and-restore/) |
| `switch.data-failed` | I | 1 | [reference-install-and-update](/pages/reference-install-and-update/) |
| `uninstall.already-absent` | I | 0 | [reference-install-update-uninstall](/pages/reference-install-update-uninstall/) |
| `update.cancelled` | I | 0 | [reference-install-update-uninstall](/pages/reference-install-update-uninstall/) |
| `update.in-progress` | I | 1 | [reference-install-update-uninstall](/pages/reference-install-update-uninstall/) |
| `update.metadata-invalid` | I | 1 | [reference-install-and-update](/pages/reference-install-and-update/) |
| `update.no-source` | I | 1 | [reference-install-and-update](/pages/reference-install-and-update/) |
| `update.review-expired` | I | 1 | [reference-install-update-uninstall](/pages/reference-install-update-uninstall/) |
| `update.superseded` | I | 1 | [reference-install-update-uninstall](/pages/reference-install-update-uninstall/) |
| `update.trial-old` | I | 1 | [reference-install-update-uninstall](/pages/reference-install-update-uninstall/) |
| `upgrade.no-path` | I | 1 | [guide-sdk-migration](/pages/guide-sdk-migration/) |

#### Registry codes (stage R)

| Code | Stage | Exit | Defined on |
|---|---|---|---|
| `mirror.closure-incomplete` | R | 1 | [reference-registry](/pages/reference-registry/) |
| `mirror.sync-busy` | R | 75 | [publish-mirror-runbook](/pages/publish-mirror-runbook/) |
| `quarantine.feed-unreachable` | R | 8 | [reference-security-response](/pages/reference-security-response/) |
| `registry.account-refused` | R | 1 | [governance](/pages/governance/) |
| `registry.builder-mismatch` | R | 1 | [reference-trust-and-supply-chain](/pages/reference-trust-and-supply-chain/) |
| `registry.bundle-invalid` | R | 1 | [reference-registry](/pages/reference-registry/) |
| `registry.countersigned` | R | 1 | [reference-registry](/pages/reference-registry/) |
| `registry.dependency-unknown` | R | 1 | [reference-registry](/pages/reference-registry/) |
| `registry.format-unsupported` | R | 1 | [reference-registry](/pages/reference-registry/) |
| `registry.known-vulnerable` | R | 1 | [publish-registry-operator-guide](/pages/publish-registry-operator-guide/) |
| `registry.last-owner` | R | 1 | [reference-plugin-distribution-cli](/pages/reference-plugin-distribution-cli/) |
| `registry.locked` | R | 1 | [guide-admin-failure-runbook](/pages/guide-admin-failure-runbook/) |
| `registry.name-reserved` | R | 1 | [reference-registry](/pages/reference-registry/) |
| `registry.namespace-not-dormant` | R | 1 | [reference-errors-and-limits](/pages/reference-errors-and-limits/) |
| `registry.not-owner` | R | 1 | [reference-registry](/pages/reference-registry/) |
| `registry.not-yanked` | R | 1 | [reference-plugin-distribution-cli](/pages/reference-plugin-distribution-cli/) |
| `registry.publisher-suspended` | R | 1 | [reference-registry](/pages/reference-registry/) |
| `registry.quarantine` | R | 1 | [reference-plugin-lifecycle-states](/pages/reference-plugin-lifecycle-states/) |
| `registry.rate-limited` | R | 8 | [reference-registry](/pages/reference-registry/) |
| `registry.read-only` | R | 8 | [publish-registry-shutdown-and-migration](/pages/publish-registry-shutdown-and-migration/) |
| `registry.reason-invalid` | R | 1 | [publish-yank-deprecate-takedown](/pages/publish-yank-deprecate-takedown/) |
| `registry.redirect-ignored` | R | 1 | [publish-registry-shutdown-and-migration](/pages/publish-registry-shutdown-and-migration/) |
| `registry.review-hash-mismatch` | R | 1 | [reference-registry](/pages/reference-registry/) |
| `registry.review-self` | R | 1 | [reference-registry](/pages/reference-registry/) |
| `registry.root-invalid` | R | 1 | [publish-registry-shutdown-and-migration](/pages/publish-registry-shutdown-and-migration/) |
| `registry.second-factor-required` | R | 1 | [reference-registry](/pages/reference-registry/) |
| `registry.secret-in-package` | R | 1 | [publish-registry-operator-guide](/pages/publish-registry-operator-guide/) |
| `registry.session-expired` | R | 1 | [reference-registry](/pages/reference-registry/) |
| `registry.source-changed` | R | 1 | [reference-trust-and-supply-chain](/pages/reference-trust-and-supply-chain/) |
| `registry.stalled` | R | 8 | [reference-registry](/pages/reference-registry/) |
| `registry.sunset-too-short` | R | 1 | [publish-registry-shutdown-and-migration](/pages/publish-registry-shutdown-and-migration/) |
| `registry.transfer-unaccepted` | R | 1 | [reference-registry](/pages/reference-registry/) |
| `registry.upload-timeout` | R | 8 | [reference-registry](/pages/reference-registry/) |
| `registry.version-order` | R | 1 | [reference-registry](/pages/reference-registry/) |
| `registry.write-failed` | R | 1 | [lifecycle](/pages/lifecycle/) |
| `registry.yanked-locked` | R | 1 | [reference-dependency-resolution](/pages/reference-dependency-resolution/) |
| `rollout.halt` | R | 1 | [guide-operator-upgrades](/pages/guide-operator-upgrades/) |
| `rollout.record-unwritable` | R | 1 | [guide-operator-upgrades](/pages/guide-operator-upgrades/) |
| `yank.reason-prefix` | R | 1 | [guide-bad-release-recall-playbook](/pages/guide-bad-release-recall-playbook/) |
| `publish.verify-failed` | R | 1 | [publish-publisher-workflow](/pages/publish-publisher-workflow/) |

#### Command-line and developer tool codes (stage C)

| Code | Stage | Exit | Defined on |
|---|---|---|---|
| `a11y.sound-only` | C | 3 | [capability-sound-play](/pages/capability-sound-play/) |
| `a11y.toast-only` | C | 3 | [capability-notify-toast](/pages/capability-notify-toast/) |
| `baseline.none-first-release` | C | 0 | [reference-plugin-cli](/pages/reference-plugin-cli/) |
| `bench.baseline-missing` | C | 1 | [guide-ci-recipes](/pages/guide-ci-recipes/) |
| `bench.noisy` | C | 3 | [perf-benchmark-workflow](/pages/perf-benchmark-workflow/) |
| `bench.regression` | C | 3 | [guide-ci-recipes](/pages/guide-ci-recipes/) |
| `bisect.state-corrupt` | C | 1 | [user-safe-mode-and-troubleshooting](/pages/user-safe-mode-and-troubleshooting/) |
| `build.bundle-failed` | C | 1 | [reference-plugin-cli](/pages/reference-plugin-cli/) |
| `build.bundle-large` | C | 1 | [typescript-sdk](/pages/typescript-sdk/) |
| `build.compile-failed` | C | 1 | [reference-plugin-cli](/pages/reference-plugin-cli/) |
| `build.component-failed` | C | 1 | [reference-plugin-cli](/pages/reference-plugin-cli/) |
| `build.component-large` | C | 1 | [rust-sdk](/pages/rust-sdk/) |
| `build.entry-missing` | C | 1 | [reference-plugin-cli](/pages/reference-plugin-cli/) |
| `build.item-newer-than-api` | C | 1 | [rust-sdk](/pages/rust-sdk/) |
| `build.locked` | C | 1 | [guide-ci-recipes](/pages/guide-ci-recipes/) |
| `build.lockfile-missing` | C | 1 | [reference-plugin-cli](/pages/reference-plugin-cli/) |
| `build.lockfile-outdated` | C | 1 | [reference-plugin-cli](/pages/reference-plugin-cli/) |
| `build.manifest-invalid` | C | 1 | [reference-plugin-cli](/pages/reference-plugin-cli/) |
| `build.manifest-missing` | C | 1 | [reference-plugin-cli](/pages/reference-plugin-cli/) |
| `build.not-reproducible` | C | 1 | [guide-reproducible-builds](/pages/guide-reproducible-builds/) |
| `build.offline-missing` | C | 1 | [reference-plugin-cli](/pages/reference-plugin-cli/) |
| `build.sdk-host-mismatch` | C | 1 | [guide-pinning-the-sdk](/pages/guide-pinning-the-sdk/) |
| `build.sdk-line-mixed` | C | 1 | [guide-pinning-the-sdk](/pages/guide-pinning-the-sdk/) |
| `build.side-undeclared` | C | 1 | [reference-plugin-cli](/pages/reference-plugin-cli/) |
| `build.type-error` | C | 1 | [reference-plugin-cli](/pages/reference-plugin-cli/) |
| `build.unpinned-input` | C | 1 | [guide-pinning-the-sdk](/pages/guide-pinning-the-sdk/) |
| `build.world-mismatch` | C | 1 | [reference-plugin-cli](/pages/reference-plugin-cli/) |
| `check.accessibility-claim` | C | 3 | [reference-manifest-schema](/pages/reference-manifest-schema/) |
| `check.behaviour-diff` | C | 3 | [reference-threat-model](/pages/reference-threat-model/) |
| `check.grant-added` | C | 0 | [reference-check-and-lint-rules](/pages/reference-check-and-lint-rules/) |
| `check.grant-missing` | C | 3 | [reference-check-and-lint-rules](/pages/reference-check-and-lint-rules/) |
| `check.grant-removed` | C | 0 | [reference-check-and-lint-rules](/pages/reference-check-and-lint-rules/) |
| `check.stale-build` | C | 0 | [reference-check-and-lint-rules](/pages/reference-check-and-lint-rules/) |
| `cli.owner-disabled` | C | 1 | [reference-cli-completion](/pages/reference-cli-completion/) |
| `cli.owner-failed` | C | 1 | [reference-cli-compatibility](/pages/reference-cli-compatibility/) |
| `component.import-ungranted` | C | 3 | [reference-check-and-lint-rules](/pages/reference-check-and-lint-rules/) |
| `component.missing` | C | 3 | [reference-check-and-lint-rules](/pages/reference-check-and-lint-rules/) |
| `config.unknown-key` | C | 2 | [reference-errors-and-limits](/pages/reference-errors-and-limits/) |
| `conformance.case-failed` | C | 2 | [reference-conformance-claims](/pages/reference-conformance-claims/) |
| `conformance.error-undocumented` | C | 3 | [reference-conformance-suite](/pages/reference-conformance-suite/) |
| `conformance.hash-mismatch` | C | 3 | [reference-conformance-claims](/pages/reference-conformance-claims/) |
| `conformance.retired-case` | C | 3 | [reference-conformance-claims](/pages/reference-conformance-claims/) |
| `conformance.wording-invalid` | C | 3 | [reference-conformance-claims](/pages/reference-conformance-claims/) |
| `debug.busy` | C | 0 | [reference-plugin-debugging](/pages/reference-plugin-debugging/) |
| `debug.hash-mismatch` | C | 1 | [reference-plugin-debugging](/pages/reference-plugin-debugging/) |
| `debug.not-loopback` | C | 1 | [reference-plugin-debugging](/pages/reference-plugin-debugging/) |
| `debug.protocol-unsupported` | C | 1 | [reference-plugin-debugging](/pages/reference-plugin-debugging/) |
| `debug.unauthenticated` | C | 1 | [reference-plugin-debugging](/pages/reference-plugin-debugging/) |
| `deps.sdk-outdated` | C | 0 | [reference-plugin-cli](/pages/reference-plugin-cli/) |
| `dev.build-failed` | C | 0 | [reference-plugin-cli](/pages/reference-plugin-cli/) |
| `dev.crash-loop` | C | 0 | [reference-plugin-lifecycle-states](/pages/reference-plugin-lifecycle-states/) |
| `dev.id-installed` | C | 1 | [reference-plugin-cli](/pages/reference-plugin-cli/) |
| `dev.watcher-degraded` | C | 0 | [reference-plugin-cli-dev-failure-modes](/pages/reference-plugin-cli-dev-failure-modes/) |
| `docs.missing-description` | C | 1 | [guide-plugin-documentation](/pages/guide-plugin-documentation/) |
| `docs.offline-missing` | C | 2 | [reference-documentation-site](/pages/reference-documentation-site/) |
| `doctor.server-running` | C | 1 | [guide-admin-failure-runbook](/pages/guide-admin-failure-runbook/) |
| `fixture.bad-value` | C | 1 | [reference-test-harness](/pages/reference-test-harness/) |
| `fixture.duplicate-id` | C | 1 | [reference-test-harness](/pages/reference-test-harness/) |
| `fixture.state-mismatch` | C | 3 | [reference-test-harness](/pages/reference-test-harness/) |
| `fixture.unknown-key` | C | 1 | [reference-test-harness](/pages/reference-test-harness/) |
| `golden.renderer-changed` | C | 3 | [reference-test-harness](/pages/reference-test-harness/) |
| `golden.theme-mismatch` | C | 3 | [reference-test-harness](/pages/reference-test-harness/) |
| `governance.comment-period-open` | C | 3 | [governance-api-change-proposals](/pages/governance-api-change-proposals/) |
| `governance.contract-change-in-patch` | C | 1 | [governance-api-change-proposals](/pages/governance-api-change-proposals/) |
| `governance.decision-missing` | C | 1 | [governance-api-change-proposals](/pages/governance-api-change-proposals/) |
| `governance.emergency-unjustified` | C | 1 | [governance-api-change-proposals](/pages/governance-api-change-proposals/) |
| `governance.proposal-missing` | C | 1 | [governance-api-change-proposals](/pages/governance-api-change-proposals/) |
| `governance.reopen-unjustified` | C | 1 | [governance-api-change-proposals](/pages/governance-api-change-proposals/) |
| `i18n.catalogue-invalid` | C | 3 | [guide-internationalisation](/pages/guide-internationalisation/) |
| `i18n.layout-overflow` | C | 3 | [guide-internationalisation](/pages/guide-internationalisation/) |
| `i18n.placeholder-mismatch` | C | 3 | [guide-internationalisation](/pages/guide-internationalisation/) |
| `i18n.reference-unresolved` | C | 3 | [guide-internationalisation](/pages/guide-internationalisation/) |
| `i18n.untranslated` | C | 0 | [guide-internationalisation](/pages/guide-internationalisation/) |
| `incident.action-unrecorded` | C | 3 | [ops-bad-release-fleet-response](/pages/ops-bad-release-fleet-response/) |
| `incident.bundle-unchecked` | C | 3 | [ops-bad-release-fleet-response](/pages/ops-bad-release-fleet-response/) |
| `incident.machine-outstanding` | C | 3 | [ops-bad-release-fleet-response](/pages/ops-bad-release-fleet-response/) |
| `inspect.file-unreadable` | C | 1 | [reference-plugin-cli](/pages/reference-plugin-cli/) |
| `inspect.hash-mismatch` | C | 1 | [reference-plugin-cli](/pages/reference-plugin-cli/) |
| `inspect.id-unknown` | C | 5 | [reference-plugin-cli](/pages/reference-plugin-cli/) |
| `inspect.no-render` | C | 1 | [reference-debugging-tools](/pages/reference-debugging-tools/) |
| `inspect.not-a-package` | C | 1 | [reference-plugin-cli](/pages/reference-plugin-cli/) |
| `matrix.host-artifact-missing` | C | 1 | [guide-backwards-compatibility-test-matrix](/pages/guide-backwards-compatibility-test-matrix/) |
| `new.dir-not-empty` | C | 1 | [reference-plugin-cli](/pages/reference-plugin-cli/) |
| `new.examples-unavailable` | C | 2 | [guide-example-plugins](/pages/guide-example-plugins/) |
| `new.id-invalid` | C | 1 | [reference-project-templates](/pages/reference-project-templates/) |
| `new.id-reserved` | C | 1 | [guide-example-plugins](/pages/guide-example-plugins/) |
| `new.license-unknown` | C | 2 | [reference-project-templates](/pages/reference-project-templates/) |
| `new.placeholder-unknown` | C | 2 | [reference-project-templates](/pages/reference-project-templates/) |
| `new.placeholder-unresolved` | C | 1 | [reference-project-templates](/pages/reference-project-templates/) |
| `new.template-cli-too-old` | C | 1 | [reference-project-templates](/pages/reference-project-templates/) |
| `new.template-fetch-failed` | C | 1 | [reference-project-templates](/pages/reference-project-templates/) |
| `new.template-invalid` | C | 1 | [reference-project-templates](/pages/reference-project-templates/) |
| `new.template-language-unsupported` | C | 1 | [reference-project-templates](/pages/reference-project-templates/) |
| `new.template-schema-unsupported` | C | 1 | [reference-project-templates](/pages/reference-project-templates/) |
| `new.template-unknown` | C | 2 | [reference-project-templates](/pages/reference-project-templates/) |
| `pack.debug-build` | C | 1 | [reference-plugin-cli](/pages/reference-plugin-cli/) |
| `pack.not-built` | C | 1 | [reference-plugin-cli](/pages/reference-plugin-cli/) |
| `pack.out-exists` | C | 1 | [reference-plugin-cli](/pages/reference-plugin-cli/) |
| `pack.requires-untested` | C | 1 | [reference-plugin-cli](/pages/reference-plugin-cli/) |
| `pack.sdk-prerelease` | C | 1 | [versioning](/pages/versioning/) |
| `pack.secret-detected` | C | 1 | [guide-publishing](/pages/guide-publishing/) |
| `pack.size-cap` | C | 1 | [reference-plugin-cli](/pages/reference-plugin-cli/) |
| `pack.stale-build` | C | 1 | [reference-plugin-cli](/pages/reference-plugin-cli/) |
| `publish.already-complete` | C | 0 | [publish-publisher-workflow](/pages/publish-publisher-workflow/) |
| `release.action-not-pinned` | C | 1 | [guide-reproducible-builds](/pages/guide-reproducible-builds/) |
| `release.assessment-missing` | C | 1 | [reference-security-testing](/pages/reference-security-testing/) |
| `release.conformance-not-packed` | C | 1 | [guide-plugin-test-strategy](/pages/guide-plugin-test-strategy/) |
| `release.tag-not-trusted` | C | 1 | [guide-reproducible-builds](/pages/guide-reproducible-builds/) |
| `replay.diverged` | C | 1 | [reference-test-harness](/pages/reference-test-harness/) |
| `replay.version-unsupported` | C | 1 | [reference-test-harness](/pages/reference-test-harness/) |
| `report.rate-limited` | C | 1 | [user-reporting-a-plugin](/pages/user-reporting-a-plugin/) |
| `report.rule-unknown` | C | 2 | [user-reporting-a-plugin](/pages/user-reporting-a-plugin/) |
| `report.text-too-long` | C | 2 | [user-reporting-a-plugin](/pages/user-reporting-a-plugin/) |
| `schema.version-newer` | C | 3 | [reference-plugin-cli](/pages/reference-plugin-cli/) |
| `sign.key-conflict` | C | 1 | [reference-signing-and-trust](/pages/reference-signing-and-trust/) |
| `sign.key-unprotected` | C | 1 | [reference-signing-and-trust](/pages/reference-signing-and-trust/) |
| `sign.no-key` | C | 1 | [reference-signing-and-trust](/pages/reference-signing-and-trust/) |
| `sign.public-key-required` | C | 1 | [reference-signing-and-trust](/pages/reference-signing-and-trust/) |
| `sign.signer-failed` | C | 1 | [reference-signing-and-trust](/pages/reference-signing-and-trust/) |
| `signature.absent` | C | 0 | [reference-check-and-lint-rules](/pages/reference-check-and-lint-rules/) |
| `signature.fingerprint` | C | 0 | [reference-check-and-lint-rules](/pages/reference-check-and-lint-rules/) |
| `signing.key-changed` | C | 1 | [reference-install-and-update](/pages/reference-install-and-update/) |
| `size.component-over-budget` | C | 0 | [reference-plugin-cli](/pages/reference-plugin-cli/) |
| `soak.lifecycle-leak` | C | 1 | [reference-host-soak-and-load-testing](/pages/reference-host-soak-and-load-testing/) |
| `state.quarantined` | C | 0 | [reference-plugin-files](/pages/reference-plugin-files/) |
| `support-bundle.exists` | C | 2 | [reference-plugin-cli](/pages/reference-plugin-cli/) |
| `template.api-missing` | C | 1 | [reference-project-templates](/pages/reference-project-templates/) |
| `template.drift` | C | 1 | [reference-project-templates](/pages/reference-project-templates/) |
| `template.drift-no-record` | C | 1 | [reference-project-templates](/pages/reference-project-templates/) |
| `template.drift-unavailable` | C | 1 | [reference-project-templates](/pages/reference-project-templates/) |
| `test.component-missing` | C | 1 | [reference-plugin-cli](/pages/reference-plugin-cli/) |
| `test.fixture-invalid` | C | 1 | [reference-plugin-cli](/pages/reference-plugin-cli/) |
| `test.flaky-persistent` | C | 3 | [guide-plugin-test-strategy](/pages/guide-plugin-test-strategy/) |
| `test.harness-mismatch` | C | 1 | [reference-plugin-test-harness](/pages/reference-plugin-test-harness/) |
| `test.migration-incomplete` | C | 3 | [reference-plugin-test-harness](/pages/reference-plugin-test-harness/) |
| `test.mismatch` | C | 3 | [reference-plugin-cli](/pages/reference-plugin-cli/) |
| `test.no-expectation` | C | 0 | [reference-test-harness](/pages/reference-test-harness/) |
| `test.no-fixtures` | C | 2 | [reference-plugin-test-harness](/pages/reference-plugin-test-harness/) |
| `test.no-match` | C | 2 | [reference-plugin-test-harness](/pages/reference-plugin-test-harness/) |
| `test.quarantine-expired` | C | 3 | [guide-plugin-test-strategy](/pages/guide-plugin-test-strategy/) |
| `test.quarantine-too-long` | C | 2 | [guide-plugin-test-strategy](/pages/guide-plugin-test-strategy/) |
| `test.report-write-failed` | C | 1 | [reference-plugin-test-harness](/pages/reference-plugin-test-harness/) |
| `test.run-timeout` | C | 1 | [reference-plugin-test-harness](/pages/reference-plugin-test-harness/) |
| `test.trap` | C | 3 | [reference-plugin-cli](/pages/reference-plugin-cli/) |
| `test.updated` | C | 0 | [reference-plugin-cli](/pages/reference-plugin-cli/) |
| `ui.bidi-control` | C | 0 | [guide-bidirectional-text-and-control-characters](/pages/guide-bidirectional-text-and-control-characters/) |
| `ui.colour-only-state` | C | 0 | [guide-colour-depth-and-no-color](/pages/guide-colour-depth-and-no-color/) |
| `ui.flash-risk` | C | 0 | [accessibility-and-theming](/pages/accessibility-and-theming/) |
| `ui.mouse-only-control` | C | 3 | [a11y-non-text-panes](/pages/a11y-non-text-panes/) |
| `ui.rgb-low-contrast` | C | 0 | [accessibility-and-theming](/pages/accessibility-and-theming/) |
| `usage.host-contract-unavailable` | C | 2 | [reference-plugin-test-harness](/pages/reference-plugin-test-harness/) |
| `usage.host-unavailable` | C | 2 | [reference-plugin-cli](/pages/reference-plugin-cli/) |
| `usage.out-of-range` | C | 2 | [reference-plugin-cli](/pages/reference-plugin-cli/) |
| `usage.rules-version-invalid` | C | 2 | [reference-check-and-lint-rules](/pages/reference-check-and-lint-rules/) |
| `usage.rules-version-unavailable` | C | 2 | [reference-check-and-lint-rules](/pages/reference-check-and-lint-rules/) |
| `usage.side-required` | C | 2 | [reference-plugin-debugging](/pages/reference-plugin-debugging/) |
| `usage.suite-unavailable` | C | 2 | [reference-conformance-suite](/pages/reference-conformance-suite/) |
| `usage.unknown-command` | C | 2 | [core-plugin-diagnostics](/pages/core-plugin-diagnostics/) |
| `usage.unknown-kind` | C | 2 | [reference-plugin-cli](/pages/reference-plugin-cli/) |
| `verify.toolchain-missing` | C | 1 | [reference-project-templates](/pages/reference-project-templates/) |

#### Toolchain codes (stage T)

| Code | Stage | Exit | Defined on |
|---|---|---|---|
| `toolchain.host-unsupported` | T | 1 | [reference-toolchain-support-matrix](/pages/reference-toolchain-support-matrix/) |
| `toolchain.pin-mismatch` | T | 4 | [reference-toolchain-support-matrix](/pages/reference-toolchain-support-matrix/) |
| `toolchain.too-old` | T | 4 | [reference-toolchain-support-matrix](/pages/reference-toolchain-support-matrix/) |

#### Host runtime codes (stage H)

| Code | Stage | Exit | Defined on |
|---|---|---|---|
| `abi.depth-exceeded` | H | - | [reference-errors-and-limits](/pages/reference-errors-and-limits/) |
| `abi.invalid-value` | H | - | [reference-runtime-model](/pages/reference-runtime-model/) |
| `abi.realloc-failed` | H | - | [reference-runtime-model](/pages/reference-runtime-model/) |
| `abi.slow-import` | H | - | [reference-runtime-model](/pages/reference-runtime-model/) |
| `activate.begin` | H | - | [reference-plugin-lifecycle-states](/pages/reference-plugin-lifecycle-states/) |
| `activate.deferred` | H | - | [reference-plugin-lifecycle-states](/pages/reference-plugin-lifecycle-states/) |
| `activate.ok` | H | - | [reference-plugin-lifecycle-states](/pages/reference-plugin-lifecycle-states/) |
| `activate.wait-expired` | H | - | [reference-plugin-lifecycle-states](/pages/reference-plugin-lifecycle-states/) |
| `activate.waiting` | H | - | [reference-plugin-lifecycle-states](/pages/reference-plugin-lifecycle-states/) |
| `agent-kit.stale` | H | - | [agent-docs-maintenance](/pages/agent-docs-maintenance/) |
| `annotate.too-many` | H | - | [reference-host-rendering](/pages/reference-host-rendering/) |
| `api.confirmation-required` | H | - | [reference-network-egress](/pages/reference-network-egress/) |
| `attach.client-limit` | H | - | [reference-persistence-and-restart](/pages/reference-persistence-and-restart/) |
| `audit.dropped` | H | - | [reference-health-and-watchdog](/pages/reference-health-and-watchdog/) |
| `audit.retention-hold` | H | - | [reference-audit-log](/pages/reference-audit-log/) |
| `auto-answer.prompt-changed` | H | - | [core-plugin-agent-detect](/pages/core-plugin-agent-detect/) |
| `auto-answer.rate-limited` | H | - | [core-plugin-agent-detect](/pages/core-plugin-agent-detect/) |
| `budget.idle-exceeded` | H | - | [reference-host-budgets](/pages/reference-host-budgets/) |
| `bundled.integrity` | H | - | [reference-trust-and-supply-chain](/pages/reference-trust-and-supply-chain/) |
| `cache.evict` | H | - | [reference-plugin-files](/pages/reference-plugin-files/) |
| `capability.denied` | H | - | [guide-plugin-test-strategy](/pages/guide-plugin-test-strategy/) |
| `ceiling.rejected` | H | - | [reference-health-and-watchdog](/pages/reference-health-and-watchdog/) |
| `cgroup.kill` | H | - | [ops-out-of-band-kill-and-stuck-server](/pages/ops-out-of-band-kill-and-stuck-server/) |
| `client-request.unavailable` | H | - | [reference-kernel-and-boot](/pages/reference-kernel-and-boot/) |
| `client.terminal-restore-failed` | H | - | [reference-persistence-and-restart](/pages/reference-persistence-and-restart/) |
| `command.alias-used` | H | - | [api-commands](/pages/api-commands/) |
| `config.parse-error` | H | - | [reference-errors-and-limits](/pages/reference-errors-and-limits/) |
| `confirm.no-client` | H | 2 | [core-plugin-worktrees](/pages/core-plugin-worktrees/) |
| `consent.no-privacy-declaration` | H | - | [ops-telemetry-consent](/pages/ops-telemetry-consent/) |
| `contribution.too-large` | H | - | [reference-host-rendering](/pages/reference-host-rendering/) |
| `contribution.too-many` | H | - | [reference-host-rendering](/pages/reference-host-rendering/) |
| `core.abi-mismatch` | H | - | [core-plugins](/pages/core-plugins/) |
| `core.data-newer` | H | - | [core-plugins](/pages/core-plugins/) |
| `core.degraded` | H | - | [reference-kernel-and-boot](/pages/reference-kernel-and-boot/) |
| `core.dependency-cycle` | H | - | [reference-kernel-and-boot](/pages/reference-kernel-and-boot/) |
| `core.integrity-failed` | H | - | [core-plugins](/pages/core-plugins/) |
| `core.owner-changed` | H | - | [extending-core-plugins](/pages/extending-core-plugins/) |
| `core.retired` | H | - | [core-plugins](/pages/core-plugins/) |
| `crash-guard.mark-unreadable` | H | - | [user-safe-mode-and-troubleshooting](/pages/user-safe-mode-and-troubleshooting/) |
| `crash-guard.unavailable` | H | - | [user-safe-mode-and-troubleshooting](/pages/user-safe-mode-and-troubleshooting/) |
| `crash-loop.exhausted` | H | - | [user-safe-mode-and-troubleshooting](/pages/user-safe-mode-and-troubleshooting/) |
| `data.keyring-unavailable` | H | - | [reference-secrets](/pages/reference-secrets/) |
| `data.network-fs` | H | - | [reference-plugin-files](/pages/reference-plugin-files/) |
| `data.root-invalid` | H | - | [reference-plugin-files](/pages/reference-plugin-files/) |
| `data.root-unresolved` | H | - | [reference-platform-support](/pages/reference-platform-support/) |
| `data.writable-again` | H | - | [reference-offline-and-low-disk](/pages/reference-offline-and-low-disk/) |
| `deactivate.deadline` | H | - | [guide-operator-upgrades](/pages/guide-operator-upgrades/) |
| `deactivate.ok` | H | - | [reference-plugin-lifecycle-states](/pages/reference-plugin-lifecycle-states/) |
| `debug.redacted` | H | - | [reference-plugin-debugging](/pages/reference-plugin-debugging/) |
| `disk.pressure` | H | - | [core-plugin-diagnostics](/pages/core-plugin-diagnostics/) |
| `engine.recycled` | H | - | [reference-health-and-watchdog](/pages/reference-health-and-watchdog/) |
| `env.unknown` | H | - | [reference-plugin-cli](/pages/reference-plugin-cli/) |
| `events.dropped` | H | - | [reference-host-telemetry](/pages/reference-host-telemetry/) |
| `events.poison` | H | - | [reference-host-scheduling](/pages/reference-host-scheduling/) |
| `extension.cap-exceeded` | H | - | [reference-errors-and-limits](/pages/reference-errors-and-limits/) |
| `extension.chain-cap` | H | - | [reference-errors-and-limits](/pages/reference-errors-and-limits/) |
| `extension.contributor-not-ready` | H | - | [reference-errors-and-limits](/pages/reference-errors-and-limits/) |
| `extension.depth-exceeded` | H | - | [reference-errors-and-limits](/pages/reference-errors-and-limits/) |
| `extension.disabled` | H | - | [reference-errors-and-limits](/pages/reference-errors-and-limits/) |
| `extension.entry-rejected` | H | - | [reference-errors-and-limits](/pages/reference-errors-and-limits/) |
| `extension.handler-timeout` | H | - | [reference-errors-and-limits](/pages/reference-errors-and-limits/) |
| `extension.not-dynamic` | H | - | [reference-errors-and-limits](/pages/reference-errors-and-limits/) |
| `extension.not-granted` | H | - | [reference-errors-and-limits](/pages/reference-errors-and-limits/) |
| `extension.reentrant` | H | - | [reference-errors-and-limits](/pages/reference-errors-and-limits/) |
| `extension.reply-not-allowed` | H | - | [reference-errors-and-limits](/pages/reference-errors-and-limits/) |
| `extension.schema-mismatch` | H | - | [reference-errors-and-limits](/pages/reference-errors-and-limits/) |
| `extension.unknown-point` | H | - | [reference-errors-and-limits](/pages/reference-errors-and-limits/) |
| `extension.veto-overridden` | H | - | [reference-errors-and-limits](/pages/reference-errors-and-limits/) |
| `extension.vetoed` | H | - | [reference-errors-and-limits](/pages/reference-errors-and-limits/) |
| `failure.core-restart` | H | - | [lifecycle](/pages/lifecycle/) |
| `failure.counted` | H | - | [reference-plugin-lifecycle-states](/pages/reference-plugin-lifecycle-states/) |
| `failure.disabled` | H | - | [reference-plugin-lifecycle-states](/pages/reference-plugin-lifecycle-states/) |
| `fs.symlink-refused` | H | - | [reference-plugin-files](/pages/reference-plugin-files/) |
| `grant.expiry-invalid` | H | 2 | [reference-grant-expiry](/pages/reference-grant-expiry/) |
| `grant.needs-interactive` | H | 3 | [reference-grant-cli-unattended](/pages/reference-grant-cli-unattended/) |
| `grant.not-held` | H | 4 | [reference-grant-cli-unattended](/pages/reference-grant-cli-unattended/) |
| `grants.unreadable` | H | - | [reference-errors-and-limits](/pages/reference-errors-and-limits/) |
| `grants.unsafe-permissions` | H | - | [guide-enterprise-deployment](/pages/guide-enterprise-deployment/) |
| `handles.rising` | H | - | [reference-health-and-watchdog](/pages/reference-health-and-watchdog/) |
| `helper.cpu-limit` | H | - | [reference-sandbox](/pages/reference-sandbox/) |
| `helper.died` | H | - | [reference-host-telemetry](/pages/reference-host-telemetry/) |
| `helper.hung` | H | - | [reference-host-helper](/pages/reference-host-helper/) |
| `helper.killed` | H | - | [reference-host-helper](/pages/reference-host-helper/) |
| `helper.protocol-error` | H | - | [reference-host-helper](/pages/reference-host-helper/) |
| `helper.spawn-failed` | H | - | [reference-host-helper](/pages/reference-host-helper/) |
| `helper.stale-call` | H | - | [reference-host-helper](/pages/reference-host-helper/) |
| `helper.too-many` | H | - | [reference-host-helper](/pages/reference-host-helper/) |
| `helper.unstable` | H | - | [reference-health-and-watchdog](/pages/reference-health-and-watchdog/) |
| `host.advisory-affected` | H | - | [reference-security-response](/pages/reference-security-response/) |
| `host.audit.sink-failed` | H | - | [reference-audit-log](/pages/reference-audit-log/) |
| `host.cache-unavailable` | H | - | [reference-host-runtime](/pages/reference-host-runtime/) |
| `host.capability-not-requested` | H | - | [reference-errors-and-limits](/pages/reference-errors-and-limits/) |
| `host.compile-timeout` | H | - | [reference-sandbox](/pages/reference-sandbox/) |
| `host.crash` | H | - | [reference-health-and-watchdog](/pages/reference-health-and-watchdog/) |
| `host.cwasm-incompatible` | H | - | [reference-host-runtime](/pages/reference-host-runtime/) |
| `host.data-readonly` | H | - | [reference-offline-and-low-disk](/pages/reference-offline-and-low-disk/) |
| `host.data-root-changed` | H | - | [reference-plugin-files](/pages/reference-plugin-files/) |
| `host.data-root-lost` | H | - | [reference-offline-and-low-disk](/pages/reference-offline-and-low-disk/) |
| `host.data-root-missing` | H | - | [reference-offline-and-low-disk](/pages/reference-offline-and-low-disk/) |
| `host.data-root-restored` | H | - | [reference-offline-and-low-disk](/pages/reference-offline-and-low-disk/) |
| `host.disk-low` | H | - | [reference-offline-and-low-disk](/pages/reference-offline-and-low-disk/) |
| `host.end-of-life` | H | - | [reference-toolchain-support-matrix](/pages/reference-toolchain-support-matrix/) |
| `host.engine-error` | H | - | [reference-host-runtime](/pages/reference-host-runtime/) |
| `host.engine-failed` | H | - | [reference-host-architecture](/pages/reference-host-architecture/) |
| `host.engine-recycled` | H | - | [reference-host-soak-and-load-testing](/pages/reference-host-soak-and-load-testing/) |
| `host.epoch-stalled` | H | - | [reference-host-runtime](/pages/reference-host-runtime/) |
| `host.essential-unstable` | H | - | [lifecycle](/pages/lifecycle/) |
| `host.format-newer` | H | - | [guide-operator-upgrades](/pages/guide-operator-upgrades/) |
| `host.health.changed` | H | - | [reference-health-and-watchdog](/pages/reference-health-and-watchdog/) |
| `host.helper-missing` | H | - | [reference-host-architecture](/pages/reference-host-architecture/) |
| `host.helper-orphan-reaped` | H | - | [reference-helper-orphan-and-server-death](/pages/reference-helper-orphan-and-server-death/) |
| `host.import-panic` | H | - | [reference-errors-and-limits](/pages/reference-errors-and-limits/) |
| `host.late` | H | - | [reference-errors-and-limits](/pages/reference-errors-and-limits/) |
| `host.loop-stalled` | H | - | [reference-health-and-watchdog](/pages/reference-health-and-watchdog/) |
| `host.memory.high` | H | - | [reference-resource-accounting](/pages/reference-resource-accounting/) |
| `host.observability.dropped` | H | - | [reference-host-telemetry](/pages/reference-host-telemetry/) |
| `host.overload-cleared` | H | - | [reference-health-and-watchdog](/pages/reference-health-and-watchdog/) |
| `host.overloaded` | H | - | [reference-health-and-watchdog](/pages/reference-health-and-watchdog/) |
| `host.queue-timeout` | H | - | [reference-host-telemetry](/pages/reference-host-telemetry/) |
| `host.shutdown-timeout` | H | - | [guide-operator-upgrades](/pages/guide-operator-upgrades/) |
| `host.shutdown.forced` | H | - | [reference-health-and-watchdog](/pages/reference-health-and-watchdog/) |
| `host.slow-calls` | H | - | [reference-host-telemetry](/pages/reference-host-telemetry/) |
| `host.slow-import` | H | - | [reference-host-telemetry](/pages/reference-host-telemetry/) |
| `host.stack-overflow` | H | - | [reference-resource-limits-matrix](/pages/reference-resource-limits-matrix/) |
| `host.start-slow` | H | - | [reference-errors-and-limits](/pages/reference-errors-and-limits/) |
| `host.synced-folder` | H | - | [reference-plugin-files](/pages/reference-plugin-files/) |
| `host.trap` | H | - | [reference-errors-and-limits](/pages/reference-errors-and-limits/) |
| `host.unclean-shutdown` | H | - | [reference-health-and-watchdog](/pages/reference-health-and-watchdog/) |
| `http-api.route-taken` | H | - | [reference-kernel-and-boot](/pages/reference-kernel-and-boot/) |
| `i18n.catalogue-too-large` | H | - | [reference-language-resolution-and-coverage](/pages/reference-language-resolution-and-coverage/) |
| `inference.unavailable` | H | - | [core-plugin-titles](/pages/core-plugin-titles/) |
| `input.paste-truncated` | H | - | [guide-text-input-and-composition](/pages/guide-text-input-and-composition/) |
| `input.paste-unterminated` | H | - | [guide-text-input-and-composition](/pages/guide-text-input-and-composition/) |
| `install.ok` | H | - | [reference-errors-and-limits](/pages/reference-errors-and-limits/) |
| `install.stale-lock` | H | - | [reference-install-update-uninstall](/pages/reference-install-update-uninstall/) |
| `instance.ended` | H | - | [reference-host-telemetry](/pages/reference-host-telemetry/) |
| `instance.started` | H | - | [reference-host-telemetry](/pages/reference-host-telemetry/) |
| `ipc.client-too-slow` | H | - | [reference-host-ipc](/pages/reference-host-ipc/) |
| `ipc.payload-too-large` | H | - | [reference-host-ipc](/pages/reference-host-ipc/) |
| `ipc.plugin-coalesced` | H | - | [reference-host-ipc](/pages/reference-host-ipc/) |
| `ipc.protocol-mismatch` | H | 76 | [reference-host-ipc](/pages/reference-host-ipc/) |
| `ipc.sequence-gap` | H | - | [reference-host-ipc](/pages/reference-host-ipc/) |
| `ipc.stale-socket-removed` | H | - | [reference-kernel-and-boot](/pages/reference-kernel-and-boot/) |
| `keybinding.kernel-reserved` | H | - | [core-plugin-keys-and-help](/pages/core-plugin-keys-and-help/) |
| `kill.policy-held` | H | 7 | [security-containment-states](/pages/security-containment-states/) |
| `layout.invalid` | H | - | [core-plugin-layout](/pages/core-plugin-layout/) |
| `load.refused` | H | - | [reference-plugin-lifecycle-states](/pages/reference-plugin-lifecycle-states/) |
| `lock.takeover` | H | - | [guide-admin-failure-runbook](/pages/guide-admin-failure-runbook/) |
| `log.cap-evict` | H | - | [reference-admin-policy](/pages/reference-admin-policy/) |
| `log.dropped` | H | - | [reference-host-telemetry](/pages/reference-host-telemetry/) |
| `log.retention-evict` | H | - | [reference-admin-policy](/pages/reference-admin-policy/) |
| `memory.pool.high` | H | - | [reference-health-and-watchdog](/pages/reference-health-and-watchdog/) |
| `migration.step` | H | - | [guide-data-migration](/pages/guide-data-migration/) |
| `net.offline` | H | 1 | [reference-cli-network-and-privacy](/pages/reference-cli-network-and-privacy/) |
| `observability.dropped` | H | - | [reference-health-and-watchdog](/pages/reference-health-and-watchdog/) |
| `plugin.abandon-pinned` | H | - | [lifecycle](/pages/lifecycle/) |
| `plugin.activated` | H | - | [reference-helper-orphan-and-server-death](/pages/reference-helper-orphan-and-server-death/) |
| `plugin.blocked` | H | - | [user-install-and-manage](/pages/user-install-and-manage/) |
| `plugin.dependency-advisory` | H | - | [reference-security-response](/pages/reference-security-response/) |
| `plugin.retired` | H | - | [reference-plugin-lifecycle-states](/pages/reference-plugin-lifecycle-states/) |
| `plugin.version-superseded` | H | - | [reference-requirements-production-completeness](/pages/reference-requirements-production-completeness/) |
| `quarantine.feed-invalid` | H | - | [reference-security-response](/pages/reference-security-response/) |
| `recovery.completed` | H | - | [reference-health-and-watchdog](/pages/reference-health-and-watchdog/) |
| `recovery.journal-corrupt` | H | - | [reference-health-and-watchdog](/pages/reference-health-and-watchdog/) |
| `restart.begin` | H | - | [reference-plugin-lifecycle-states](/pages/reference-plugin-lifecycle-states/) |
| `restart.cancelled` | H | - | [reference-plugin-lifecycle-states](/pages/reference-plugin-lifecycle-states/) |
| `restart.scheduled` | H | - | [reference-plugin-lifecycle-states](/pages/reference-plugin-lifecycle-states/) |
| `runtime.dir-fallback` | H | - | [reference-kernel-and-boot](/pages/reference-kernel-and-boot/) |
| `runtime.dir-unsafe` | H | - | [reference-kernel-and-boot](/pages/reference-kernel-and-boot/) |
| `runtime.socket-path-too-long` | H | - | [reference-kernel-and-boot](/pages/reference-kernel-and-boot/) |
| `sandbox.unavailable` | H | - | [reference-sandbox](/pages/reference-sandbox/) |
| `screen.occlusion` | H | - | [porting-iliumanim](/pages/porting-iliumanim/) |
| `secret.expired` | H | - | [reference-plugin-files](/pages/reference-plugin-files/) |
| `secret.host-mismatch` | H | - | [reference-settings-schema](/pages/reference-settings-schema/) |
| `server.starting` | H | - | [reference-host-architecture](/pages/reference-host-architecture/) |
| `session.unresponsive` | H | 6 | [reference-plugin-cli](/pages/reference-plugin-cli/) |
| `session.unresponsive-fallback` | H | 0 | [reference-plugin-cli](/pages/reference-plugin-cli/) |
| `settings.anchor-missing` | H | - | [reference-errors-and-limits](/pages/reference-errors-and-limits/) |
| `settings.invalid-value` | H | - | [reference-errors-and-limits](/pages/reference-errors-and-limits/) |
| `settings.key-not-owned` | H | - | [reference-errors-and-limits](/pages/reference-errors-and-limits/) |
| `settings.section-dropped` | H | - | [reference-errors-and-limits](/pages/reference-errors-and-limits/) |
| `settings.tab-dropped` | H | - | [reference-errors-and-limits](/pages/reference-errors-and-limits/) |
| `settings.unknown-key` | H | - | [reference-errors-and-limits](/pages/reference-errors-and-limits/) |
| `settings.value-dropped` | H | - | [reference-plugin-cli](/pages/reference-plugin-cli/) |
| `settings.value-ignored` | H | - | [reference-errors-and-limits](/pages/reference-errors-and-limits/) |
| `settings.value-reset` | H | - | [reference-settings-schema](/pages/reference-settings-schema/) |
| `snapshot.restored-previous` | H | - | [reference-persistence-and-restart](/pages/reference-persistence-and-restart/) |
| `snapshot.section-too-large` | H | - | [reference-config-and-data-migration](/pages/reference-config-and-data-migration/) |
| `snapshot.state-too-large` | H | - | [reference-persistence-and-restart](/pages/reference-persistence-and-restart/) |
| `snapshot.write-failed` | H | - | [reference-persistence-and-restart](/pages/reference-persistence-and-restart/) |
| `state.temp-removed` | H | - | [reference-plugin-files](/pages/reference-plugin-files/) |
| `state.too-old` | H | - | [host-upgrade-while-plugins-run](/pages/host-upgrade-while-plugins-run/) |
| `storage.bucket-in-use` | H | 1 | [api-storage](/pages/api-storage/) |
| `storage.corrupt` | H | 1 | [api-storage](/pages/api-storage/) |
| `storage.full` | H | - | [reference-errors-and-limits](/pages/reference-errors-and-limits/) |
| `storage.key-not-found` | H | 1 | [api-storage](/pages/api-storage/) |
| `storage.reset` | H | - | [api-storage](/pages/api-storage/) |
| `store.version` | H | - | [reference-errors-and-limits](/pages/reference-errors-and-limits/) |
| `theme.low-contrast` | H | - | [accessibility-and-theming](/pages/accessibility-and-theming/) |
| `timer.clock-jump` | H | - | [reference-trust-and-supply-chain](/pages/reference-trust-and-supply-chain/) |
| `topic.version-unsupported` | H | - | [api-messages-and-topics](/pages/api-messages-and-topics/) |
| `trace.unredacted-denied` | C | 1 | [reference-debugging-tools](/pages/reference-debugging-tools/) |
| `trap.abi` | H | - | [reference-errors-and-limits](/pages/reference-errors-and-limits/) |
| `trap.deadline` | H | - | [reference-plugin-debugging](/pages/reference-plugin-debugging/) |
| `trap.invalid-ui-tree` | H | - | [reference-errors-and-limits](/pages/reference-errors-and-limits/) |
| `trap.memory` | H | - | [reference-errors-and-limits](/pages/reference-errors-and-limits/) |
| `trap.panic` | H | - | [testing](/pages/testing/) |
| `trap.submission-batch` | H | - | [reference-errors-and-limits](/pages/reference-errors-and-limits/) |
| `trap.table` | H | - | [reference-errors-and-limits](/pages/reference-errors-and-limits/) |
| `tree.pane-limit` | H | - | [core-plugin-tree](/pages/core-plugin-tree/) |
| `trust.unreadable` | H | - | [reference-errors-and-limits](/pages/reference-errors-and-limits/) |
| `ui.contrast-adjusted` | H | - | [guide-colour-depth-and-no-color](/pages/guide-colour-depth-and-no-color/) |
| `ui.duplicate-key` | H | - | [reference-host-rendering](/pages/reference-host-rendering/) |
| `ui.flash-limited` | H | - | [core-plugin-ambient](/pages/core-plugin-ambient/) |
| `ui.invalid-tree` | H | - | [reference-host-rendering](/pages/reference-host-rendering/) |
| `ui.invalidate-in-render` | H | - | [reference-host-rendering](/pages/reference-host-rendering/) |
| `ui.label-missing` | H | - | [a11y-screen-reader-and-localized-text](/pages/a11y-screen-reader-and-localized-text/) |
| `ui.motion-ignored` | H | - | [core-plugin-ambient](/pages/core-plugin-ambient/) |
| `ui.render-timeout` | H | - | [reference-host-rendering](/pages/reference-host-rendering/) |
| `ui.text-sanitized` | H | - | [ui-model](/pages/ui-model/) |
| `ui.too-deep` | H | - | [reference-host-rendering](/pages/reference-host-rendering/) |
| `ui.too-large` | H | - | [reference-host-rendering](/pages/reference-host-rendering/) |
| `ui.widget-not-allowed` | H | - | [reference-host-rendering](/pages/reference-host-rendering/) |
| `voice.low-disk` | H | - | [core-plugin-voice](/pages/core-plugin-voice/) |
| `worktrees.branch-exists` | H | - | [core-plugin-worktrees](/pages/core-plugin-worktrees/) |
| `worktrees.denied` | H | - | [core-plugin-worktrees](/pages/core-plugin-worktrees/) |
| `worktrees.dirty` | H | - | [core-plugin-worktrees](/pages/core-plugin-worktrees/) |
| `worktrees.git-failed` | H | - | [core-plugin-worktrees](/pages/core-plugin-worktrees/) |
| `worktrees.in-use` | H | - | [core-plugin-worktrees](/pages/core-plugin-worktrees/) |
| `worktrees.not-a-repository` | H | - | [core-plugin-worktrees](/pages/core-plugin-worktrees/) |
| `worktrees.not-found` | H | - | [core-plugin-worktrees](/pages/core-plugin-worktrees/) |
| `worktrees.refused` | H | - | [core-plugin-worktrees](/pages/core-plugin-worktrees/) |
| `worktrees.vetoed` | H | - | [core-plugin-worktrees](/pages/core-plugin-worktrees/) |

#### Families catalogued on another page

| Family | Stage | Exit | Catalogue |
|---|---|---|---|
| `lint.*` | C | 3 for a finding of severity `error`, or of any severity under `--deny warnings`; else 0 | The rule tables of [Check and lint rules](/pages/reference-check-and-lint-rules/). Each rule id is one code |
| `harness.*` | C | 3 | The table of [Test harness](/pages/reference-test-harness/). Printed by `ilium plugin test` only |

A diagnostic code is a dotted name in code type (`family.name`, lower-case letters, digits and hyphens, two or more parts) that a page uses as the code of an error, a refusal, a warning, a check or lint finding, a notice, an alert or a log line. A dotted name is not a diagnostic code when it resolves in one of the registries below. The release gate reads the same registries, so that every dotted name of the manual resolves in exactly one place.

| Registry | Holds | Example |
|---|---|---|
| This catalogue, the retired table and the two family tables above | Diagnostic codes | `install.file-busy` |
| [WIT reference](/pages/reference-wit/) | Host functions and types, written `interface.name` | `storage.get` |
| [Capabilities reference](/pages/reference-capabilities/) | Grants | `tree.read` |
| [Manifest reference](/pages/reference-manifest/), [Settings schema reference](/pages/reference-settings-schema/), [Admin policy](/pages/reference-admin-policy/) | Manifest, settings and policy keys and their paths | `requires.plugins` |
| [Audit log](/pages/reference-audit-log/) | Audit event names | `rollout.held` |
| [Core extension points](/pages/reference-core-extension-points/) and each plugin manifest | Plugin ids and the commands, services, topics and points below them | `ilium.core.tree` |
| [Host telemetry](/pages/reference-host-telemetry/) and the requirement that defines a counter | Metric and counter names | `plugin.focus.dropped` |
| none | File names, host names and version numbers | `manifest.json` |

- **[PRM-108]** The system must list in this catalogue, or for the `lint.*` and `harness.*` families in the rule table that this catalogue names, every diagnostic code that a page of the manual names, each with its stage, its exit code and the page that states its cause and fix. A release gate must extract every dotted name in code type from the manual pages and every code string from the host binary, must resolve each name in the registries of the table above, and must fail naming each name that resolves in no registry, each code that the catalogue does not hold or whose stage or exit code differs from the binary, and must fail when the count stated in the catalogue text differs from the number of rows.

## Retired and orphan codes

Older pages use the spellings below. They are not printed. A host that reads old logs maps them to the code on the right.

| Retired code | Use instead | Note |
|---|---|---|
| `manifest.key-unknown` | `manifest.unknown-key` | Renamed. |
| `manifest.zip-path` | `package.path-invalid` | The hardened reader uses the package family. |
| `manifest.zip-duplicate` | `package.path-duplicate` | Same. |
| `manifest.zip-entry-type` | `package.entry-symlink or package.entry-directory` | Same. |
| `manifest.zip-format` | `package.zip-invalid` | Same. |
| `manifest.key-declined` | `trust.declined` | A declined key is a trust outcome. |
| `install.trust-declined` | `trust.declined` | Same. |
| `manifest.publisher-key-changed` | `package.publisher-key-changed` | Package family. |
| `manifest.package-blocked` | `package.blocked` | Block list. |
| `manifest.memory-over-ceiling` | none | A limit above the ceiling is clamped and logged, never refused. |
| `manifest.replaces-surface` | `manifest.replaces-surface-incomplete` | Renamed. |
| `manifest.settings-page-clash` | none | Settings pages were removed. |
| `package.api-mismatch` | `manifest.api-version` | Renamed. |
| `package.inventory-mismatch` | `manifest.file-unlisted, manifest.file-missing` | Split. |
| `package.hash` | `manifest.file-hash` | Renamed. |
| `package.hash-mismatch` | `manifest.file-hash (at install), package.corrupt (at load)` | Split. |
| `package.layout` | `package.manifest-missing or package.entry-unexpected` | Split. |
| `policy.tier` | `policy.tier-denied` | Detail code of policy.blocked. |
| `policy.id` | `policy.id-denied` | Same. |
| `policy.publisher` | `policy.publisher-denied` | Same. |
| `policy.grant` | `policy.grant-forbidden` | Detail shown next to the locked row. A denied grant never fails an install. |
| `policy.source` | `policy.source-denied` | The source (registry URL, mirror or local file) is not in the admin `allowed_sources` list. |
| `policy.json` | `policy.invalid` | Same. |
| `trust.unsigned` | `install.unsigned` | An unsigned package without `--allow-unsigned` is an install refusal at stage 8. |
| `trust.key-permissions` | `sign.key-permissions` | Only `sign` reads the secret key file. |
| `install.declined` | `install.review-declined` | Same meaning. Both appear in older pages; new text uses install.review-declined. |
| `install.disk-full` | `install.no-space (before writing), install.io (during)` | The check before writing is install.no-space. |
| `signature.invalid` | `manifest.signature-invalid` | Install pipeline text of older pages. |
| `signature.unknown-key` | `trust.key-unknown` | Same. |
| `signature.revoked` | `trust.key-revoked` | Same. |
| `tier.not-allowed` | `policy.tier-denied` | Same. |
| `api.window` | `package.api-unsupported` | Same. |
| `rollback.no-previous` | `install.no-previous` | Same. |
| `rollback.data-newer` | `migration.data-newer` | Same. |
| `install.same-version` | `package.version-reused or install.already-installed` | A different manifest hash is `package.version-reused`; an equal one is `install.already-installed`. |
| `install.same-version-different-content` | `package.version-reused` | Same. |

| Id | Rule |
|---|---|
| [PKG-577] | A host MUST NOT print a retired code. A test that expects a retired code is wrong and MUST be updated to the replacement. |
| [PKG-581] | The host MUST refuse an install whose package source (registry URL, mirror or local file) is not in the administrator `allowed_sources` list with `policy.blocked` (detail `policy.source-denied`). |
| [PKG-582] | A `cli` contribution that names a name owned by the kernel itself (plugin, session, kill-session, help, version) MUST be refused with `manifest.cli-name-reserved`. The names `recap`, `progress`, `wait`, `chat`, `panes`, `broadcast`, `new-pane` and `voice` are owned by bundled core plugins: only that core plugin may declare its own, and any other plugin is refused with `manifest.cli-name-taken`, which is used for a name that another installed plugin owns. |
| [PKG-583] | An extension point that declares a risk level lower than the grant its handlers receive, or an unknown risk, MUST be refused with `manifest.point-risk`. |

## Edge cases

| Situation | Result |
|---|---|
| The manifest is `{}` | Stage 3 prints `manifest.key-missing` for `api`, `id`, `name`, `version` and `publisher`, and `manifest.schema` for the missing component |
| The manifest is a JSON array | `manifest.schema` at pointer `` (the root) |
| A number is `1.0` where an integer is needed | `manifest.schema`. A JSON number with a fraction is not an integer |
| A string has a BOM at the start of the file | `manifest.json-invalid` |
| A key repeats | `manifest.duplicate-key` at stage 2. No later stage runs |
| `files` is present in a source manifest | `pack` replaces it. `check` ignores it |
| An error has no pointer | The `pointer` field is omitted |

## See also

- [Manifest reference](/pages/reference-manifest/)
- [Package format](/pages/reference-package-format/)
- [Signing and trust](/pages/reference-signing-and-trust/)
- [Registry reference](/pages/reference-registry/)
- [Versions and dependencies](/pages/reference-dependency-resolution/)
- [Install and update](/pages/reference-install-and-update/)
- [Error codes and limits](/pages/reference-errors-and-limits/)

## Requirement statements

- **[PKG-560]** The schema document MUST declare `$schema` as `https://json-schema.org/draft/2020-12/schema` and `$id` as `urn:ilium:schema:plugin-manifest:0.1.0`. It MUST NOT use a keyword outside the 2020-12 core and validation vocabularies.
- **[PKG-561]** `ilium plugin check` and the installer MUST accept exactly the manifests that this schema accepts at stage 3, and MUST apply the extra rules of the list below at stages 3 and 4. A manifest that the schema refuses MUST be refused with the code of the failing key (see the mapping in PKG-562).
- **[PKG-562]** The following MUST hold. A schema failure maps to a code in this order: a property outside `properties` gives `manifest.unknown-key`; a missing `required` property gives `manifest.key-missing`; a failing `pattern` on `id` gives `manifest.id-invalid`; a failing `pattern` on `version` gives `manifest.version-invalid`; a faili
- **[PKG-563]** The schema MUST NOT be fetched from the network. A copy ships with the host and with `ilium plugin check`, and `ilium plugin check --print-schema` writes it to standard output.
- **[PKG-564]** The installer MUST refuse with `manifest.id-reserved` an `id` that starts with `ilium.` or that equals a reserved publisher id of PKG-220..
- **[PKG-565]** The installer MUST refuse this input with `manifest.exports-mismatch`: the `exports` list of a component matches the exports of the real component..
- **[PKG-566]** When each grant exists in this API version, its argument follows the grammar and it is available on its side., the client MUST do this: `manifest.grant-unknown` and the grant codes
- **[PKG-567]** The installer MUST refuse this input with `manifest.when-syntax`, `manifest.settings-key-clash`: `when` and `visible_when` expressions parse, and settings keys are unique..
- **[PKG-568]** When ids are unique inside each contribution list, and commands, topics and services follow the prefix rules of their pages (a services or commands name may also use the id of a core plugin named in this package's replaces list, for items in that core plugin's surface)., the client MUST do this: the code of the contribution
- **[PKG-569]** The installer MUST refuse this input with `manifest.requires-invalid`, `manifest.requires-self`: `requires` ranges parse, and `requires.plugins` holds no duplicate and not the plugin itself..
- **[PKG-570]** The installer MUST refuse this input with `manifest.file-missing`: the side of each declared component has its file in the archive, and each file in `help/` and `sounds/` named by a contribution exists..
- **[PKG-571]** The following MUST hold. Every error is one JSONL line with `type` `error`, a `code` from this page, a `message` built from the template, a `stage` (number or letter as above) and, when a place exists, a `pointer` (RFC 6901) or a `path`.
- **[PKG-572]** A template placeholder in braces MUST be replaced with the value. A template MUST NOT contain a secret, a token or the content of a file outside the package.
- **[PKG-573]** The following MUST hold. The `message` is English text. The `code` is the stable interface: tools and tests match the code and never the message.
- **[PKG-574]** The following MUST hold. When several errors occur, the exit code is that of the first error printed.
- **[PKG-578]** The following MUST hold. Within a stage, errors are printed in the order of the central directory entry they concern (errors without an entry first), and for one entry in the order of the check list of the stage. Manifest errors sort by JSON pointer in byte order. A conformance row that names an expected result lists every code it expects, in print order, not only the first.
- **[PKG-575]** A host MUST NOT print a code that is not in this catalogue. A new code needs an entry here first.
- **[PKG-576]** Every code of this catalogue MUST appear in at least one test of the conformance suite with the stage and the exit code shown.
- **[PKG-577]** A host MUST NOT print a retired code. A test that expects a retired code is wrong and MUST be updated to the replacement.
- **[PKG-581]** The host MUST refuse an install whose package source (registry URL, mirror or local file) is not in the administrator `allowed_sources` list with `policy.blocked` (detail `policy.source-denied`).
- **[PKG-582]** A `cli` contribution that names a name owned by the kernel itself (plugin, session, kill-session, help, version) MUST be refused with `manifest.cli-name-reserved`. The names `recap`, `progress`, `wait`, `chat`, `panes`, `broadcast`, `new-pane` and `voice` are owned by bundled core plugins: only that core plugin may declare its own, and any other plugin is refused with `manifest.cli-name-taken`, which is used for a name that another installed plugin owns.
- **[PKG-583]** An extension point that declares a risk level lower than the grant its handlers receive, or an unknown risk, MUST be refused with `manifest.point-risk`.

## Production additions (round 12)

- **[PRP-004]** The system must accept an optional https support_url of at most 256 characters in the manifest, show it as a distinct Get help link in Settings, the store listing and ilium plugin info, warn manifest.support-missing when neither support_url nor repository is set, and open it only after the open-external consent dialog.
