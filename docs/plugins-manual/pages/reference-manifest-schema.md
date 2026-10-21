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
              "actions": {"type": "array", "items": {"type": "object"}, "maxItems": 32},
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
          "maxItems": 4
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

Exit codes are those of [Distribution commands](/pages/reference-plugin-distribution-cli/#exit-codes). The column Exit below gives the exit code of the command that prints the code.

| Id | Rule |
|---|---|
| [PKG-571] | Every error is one JSONL line with `type` `error`, a `code` from this page, a `message` built from the template, a `stage` (number or letter as above) and, when a place exists, a `pointer` (RFC 6901) or a `path`. |
| [PKG-572] | A template placeholder in braces MUST be replaced with the value. A template MUST NOT contain a secret, a token or the content of a file outside the package. |
| [PKG-573] | The `message` is English text. The `code` is the stable interface: tools and tests match the code and never the message. |
| [PKG-574] | When several errors occur, the exit code is that of the first error printed. |
| [PKG-578] | Within a stage, errors are printed in the order of the central directory entry they concern (errors without an entry first), and for one entry in the order of the check list of the stage. Manifest errors sort by JSON pointer in byte order. A conformance row that names an expected result lists every code it expects, in print order, not only the first. |

## Error catalogue

This catalogue has 251 codes, of which 93 are in the `manifest.*` family. A code that appears in an older page and is not here is in the table of retired codes below.

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
| `manifest.settings-glyph` | 3 | 1 | `{pointer}: "{value}" is not a single-cell glyph.` |
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
| `install.cancelled` | 11 | 1 | `The review was closed.` |
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
| `session.not-found` | C | 5 | `No session named {name}.` |
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
