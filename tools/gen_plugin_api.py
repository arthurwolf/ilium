#!/usr/bin/env python3
"""Generate the Rust mirror types of the WIT contract from wit/.

Input: the wit/ tree written by tools/extract_wit.py.
Output, for each crate in CRATES:

    <crate>/src/generated/mod.rs
    <crate>/src/generated/{types,host,primitives,anim,exports}.rs

The two crates (ilium-plugin-api for the host, ilium-plugin-sdk-core for the
guest) may not depend on each other, so both get the same text. The files of
the two crates are byte-identical apart from the second line, which names the
output path. The first line is "// @generated from wit/ sha256=<hash>", where
<hash> is the tree hash defined in wit_tree_sha256().

Extra output for ilium-plugin-api only:

    ilium-plugin-api/tests/abi/generated_roundtrip.rs  serde round trip of every generated type
    ilium-plugin-api/tests/abi/golden/generated.sha256  sha256 of each generated file (drift golden)

Mapping (test plan 1.3, generator contract):

    record        struct, pub fields, derive(Debug, Clone, PartialEq, Serialize, Deserialize)
                  (+ Default when every field has a default), serde kebab-case names
    enum          fieldless enum, also Copy, Eq, Hash; CASE_NAMES and as_str()
    variant       enum with tuple payloads; CASE_NAMES and case_name()
    flags         u32 bit-flag newtype with bit-flags style operators
    resource      opaque handle newtype <Name>Handle(pub u32); methods become trait
                  methods "<resource>_<method>" that take the handle first;
                  a constructor becomes "<resource>_new"
    type in ids   #[repr(transparent)] id newtype (plugin-id wraps String)
    other type    pub type alias
    functions     trait Host<Interface> (ilium:host, ilium:primitives, ilium:anim
                  imports, implemented by the host) or Export<Interface>
                  (ilium:exports, implemented by the guest); receiver &mut self

Every interface becomes a Rust module named after it, so equal names in two
interfaces never clash. Each package module re-exports every name that is
defined in exactly one of its interfaces.

Python 3 standard library only. JSONL on stdout, one object per line with a
"type" field. Exit codes: 0 success (or no drift with --check), 1 drift with
--check, 2 usage or parse error.
"""

import argparse
import hashlib
import json
import pathlib
import re
import shutil
import subprocess
import sys

CRATES = ("ilium-plugin-api", "ilium-plugin-sdk-core")
ROUNDTRIP_CRATE = "ilium-plugin-api"
PACKAGE_ORDER = ("types", "host", "primitives", "anim", "exports")
PACKAGE_OF_DIRECTORY = {
    "types": "ilium:types",
    "host": "ilium:host",
    "primitives": "ilium:primitives",
    "anim": "ilium:anim",
    "exports": "ilium:exports",
}
SECTION_OF_PACKAGE = {"types": 4, "host": 5, "primitives": 6, "anim": 7, "exports": 8}
WORLD_PACKAGES = ("server", "client", "anim")
VERSION = "0.1.0"

RUST_KEYWORDS = {
    "as", "break", "const", "continue", "crate", "else", "enum", "extern", "false", "fn", "for",
    "if", "impl", "in", "let", "loop", "match", "mod", "move", "mut", "pub", "ref", "return",
    "self", "Self", "static", "struct", "super", "trait", "true", "type", "unsafe", "use",
    "where", "while", "async", "await", "dyn", "abstract", "become", "box", "do", "final",
    "macro", "override", "priv", "typeof", "unsized", "virtual", "yield", "try", "gen",
}
NOT_RAW = {"self", "Self", "super", "crate"}
RESERVED_TYPE_NAMES = {
    "Option", "Result", "String", "Vec", "Box", "Some", "None", "Ok", "Err", "Default", "Copy",
    "Clone", "Send", "Sync", "Sized", "Drop", "Fn", "Iterator", "ToString", "ToOwned",
}
PRIMITIVES = {
    "u8": "u8", "u16": "u16", "u32": "u32", "u64": "u64",
    "s8": "i8", "s16": "i16", "s32": "i32", "s64": "i64",
    "f32": "f32", "f64": "f64", "bool": "bool", "char": "char", "string": "String",
}
CLIPPY_ALLOW = (
    "clippy::large_enum_variant",
    "clippy::too_many_arguments",
    "clippy::type_complexity",
    "clippy::enum_variant_names",
    "clippy::doc_lazy_continuation",
)


def emit(record):
    """Print one JSONL record."""
    print(json.dumps(record, sort_keys=True))


class GenerateError(Exception):
    """A problem in wit/ that stops the generation."""


# ---------------------------------------------------------------- tokenizer

TOKEN_PATTERN = re.compile(
    r"""
    (?P<doc>///[^\n]*)
  | (?P<comment>//[^\n]*)
  | (?P<space>\s+)
  | (?P<version>\d+\.\d+\.\d+)
  | (?P<arrow>->)
  | (?P<ident>%?[a-z][a-z0-9]*(?:-[a-z0-9]+)*)
  | (?P<number>\d+)
  | (?P<punct>[{}()<>,;:=./@_*])
    """,
    re.VERBOSE,
)
UNSTABLE_PATTERN = re.compile(r"^//\s*@unstable\(feature\s*=\s*([a-z0-9-]+)\)\s*$")


class Token:
    """One lexical token with its line for error messages."""

    def __init__(self, kind, text, line):
        self.kind = kind
        self.text = text
        self.line = line

    def __repr__(self):
        return f"{self.kind}:{self.text!r}@{self.line}"


def tokenize(text, source):
    """Split WIT text into tokens. Doc comments and @unstable markers are kept as tokens."""
    tokens = []
    position = 0
    line = 1
    while position < len(text):
        match = TOKEN_PATTERN.match(text, position)
        if not match:
            raise GenerateError(f"{source}:{line}: unexpected character {text[position]!r}")
        kind = match.lastgroup
        value = match.group(kind)
        if kind == "doc":
            tokens.append(Token("doc", value[3:].strip(), line))
        elif kind == "comment":
            unstable = UNSTABLE_PATTERN.match(value.strip())
            if unstable:
                tokens.append(Token("unstable", unstable.group(1), line))
        elif kind != "space":
            tokens.append(Token(kind, value, line))
        line += value.count("\n")
        position = match.end()
    tokens.append(Token("eof", "", line))
    return tokens


# ---------------------------------------------------------------- parser

class Parser:
    """Recursive-descent parser for the subset of WIT that the contract uses."""

    def __init__(self, tokens, source):
        self.tokens = tokens
        self.index = 0
        self.source = source

    def peek(self, offset=0):
        return self.tokens[self.index + offset]

    def next(self):
        token = self.tokens[self.index]
        self.index += 1
        return token

    def fail(self, message):
        raise GenerateError(f"{self.source}:{self.peek().line}: {message} (at {self.peek()!r})")

    def expect(self, text):
        token = self.next()
        if token.text != text:
            self.index -= 1
            self.fail(f"expected {text!r}")
        return token

    def accept(self, text):
        if self.peek().text == text and self.peek().kind in ("punct", "ident", "arrow"):
            self.index += 1
            return True
        return False

    def identifier(self):
        token = self.next()
        if token.kind != "ident":
            self.index -= 1
            self.fail("expected an identifier")
        return token.text.lstrip("%")

    def leading(self):
        """Collect doc lines and an @unstable feature before an item."""
        docs = []
        unstable = None
        while self.peek().kind in ("doc", "unstable"):
            token = self.next()
            if token.kind == "doc":
                docs.append(token.text)
            else:
                unstable = token.text
        return docs, unstable

    def parse_file(self):
        package = None
        items = []
        while True:
            docs, unstable = self.leading()
            token = self.peek()
            if token.kind == "eof":
                break
            if token.text == "package":
                self.next()
                package = self.package_path()
                self.expect(";")
                continue
            if token.text == "interface":
                self.next()
                name = self.identifier()
                items.append(self.interface_body(name, docs))
                continue
            if token.text == "world":
                self.next()
                name = self.identifier()
                items.append(self.world_body(name, docs))
                continue
            self.fail("expected package, interface or world")
        return package, items

    def package_path(self):
        namespace = self.identifier()
        self.expect(":")
        name = self.identifier()
        self.expect("@")
        version = self.next()
        if version.kind != "version":
            self.fail("expected a version")
        return f"{namespace}:{name}@{version.text}"

    def use_path(self):
        """Parse "ns:pkg/iface@ver" or "iface". Returns (package or None, interface)."""
        first = self.identifier()
        if self.accept(":"):
            package = self.identifier()
            self.expect("/")
            interface = self.identifier()
            self.expect("@")
            version = self.next()
            if version.kind != "version":
                self.fail("expected a version")
            return f"{first}:{package}@{version.text}", interface
        return None, first

    def world_body(self, name, docs):
        self.expect("{")
        entries = []
        while not self.accept("}"):
            self.leading()
            keyword = self.identifier()
            if keyword not in ("import", "export", "include"):
                self.fail("expected import, export or include")
            package, interface = self.use_path()
            self.expect(";")
            entries.append({"kind": keyword, "package": package, "interface": interface})
        return {"kind": "world", "name": name, "docs": docs, "entries": entries}

    def interface_body(self, name, docs):
        self.expect("{")
        interface = {
            "kind": "interface", "name": name, "docs": docs, "uses": [], "types": [], "functions": [],
        }
        while True:
            item_docs, unstable = self.leading()
            if self.accept("}"):
                break
            token = self.peek()
            if token.text == "use":
                self.next()
                package, source_interface = self.use_path()
                self.expect(".")
                self.expect("{")
                names = []
                while not self.accept("}"):
                    imported = self.identifier()
                    alias = imported
                    if self.accept("as"):
                        alias = self.identifier()
                    names.append((imported, alias))
                    self.accept(",")
                self.expect(";")
                interface["uses"].append({"package": package, "interface": source_interface, "names": names})
                continue
            if token.text in ("record", "enum", "flags", "variant", "type", "resource") and self.peek(1).kind == "ident" and self.peek(2).text != ":":
                keyword = self.next().text
                definition = self.parse_definition(keyword)
                definition["docs"] = item_docs
                definition["unstable"] = unstable
                interface["types"].append(definition)
                continue
            if token.kind == "ident" and self.peek(1).text == ":":
                function = self.parse_function()
                function["docs"] = item_docs
                function["unstable"] = unstable
                interface["functions"].append(function)
                continue
            self.fail("expected a type, a use or a function")
        return interface

    def parse_record(self):
        name = self.identifier()
        self.expect("{")
        fields = []
        while True:
            docs, _ = self.leading()
            if self.accept("}"):
                break
            field_name = self.identifier()
            self.expect(":")
            fields.append({"name": field_name, "type": self.parse_type(), "docs": docs})
            if not self.accept(","):
                self.leading()
                self.expect("}")
                break
        return {"kind": "record", "name": name, "fields": fields}

    def parse_cases(self, kind, allow_payload):
        name = self.identifier()
        self.expect("{")
        cases = []
        while True:
            docs, unstable = self.leading()
            if self.accept("}"):
                break
            case_name = self.identifier()
            payload = None
            if allow_payload and self.accept("("):
                payload = self.parse_type()
                self.expect(")")
            cases.append({"name": case_name, "payload": payload, "docs": docs, "unstable": unstable})
            if not self.accept(","):
                self.leading()
                self.expect("}")
                break
        return {"kind": kind, "name": name, "cases": cases}

    def parse_type(self):
        token = self.peek()
        if token.text == "_" and token.kind == "punct":
            self.next()
            return ("unit",)
        name = self.identifier()
        if name in PRIMITIVES:
            return ("primitive", name)
        if name in ("list", "option", "borrow", "own"):
            self.expect("<")
            inner = self.parse_type()
            self.expect(">")
            return (name, inner)
        if name == "tuple":
            self.expect("<")
            members = [self.parse_type()]
            while self.accept(","):
                members.append(self.parse_type())
            self.expect(">")
            return ("tuple", tuple(members))
        if name == "result":
            ok = ("unit",)
            error = ("unit",)
            if self.accept("<"):
                ok = self.parse_type()
                if self.accept(","):
                    error = self.parse_type()
                self.expect(">")
            return ("result", ok, error)
        return ("named", name)

    def parse_definition(self, keyword):
        """Parse the body of a type definition that starts with `keyword`."""
        if keyword == "record":
            return self.parse_record()
        if keyword == "enum":
            return self.parse_cases("enum", allow_payload=False)
        if keyword == "flags":
            return self.parse_cases("flags", allow_payload=False)
        if keyword == "variant":
            return self.parse_cases("variant", allow_payload=True)
        if keyword == "resource":
            return self.parse_resource()
        if keyword == "type":
            return self.parse_alias()
        self.fail(f"unknown definition keyword {keyword}")
        return None

    def parse_alias(self):
        """`type name = ty;`"""
        name = self.identifier()
        self.expect("=")
        target = self.parse_type()
        self.expect(";")
        return {"kind": "type", "name": name, "target": target}

    def parse_resource(self):
        name = self.identifier()
        methods = []
        if self.accept(";"):
            return {"kind": "resource", "name": name, "methods": methods}
        self.expect("{")
        while True:
            docs, unstable = self.leading()
            if self.accept("}"):
                break
            if self.peek().text == "constructor":
                self.next()
                self.expect("(")
                parameters = self.parse_parameters()
                self.expect(";")
                methods.append({"name": "constructor", "kind": "constructor", "parameters": parameters,
                                "result": None, "docs": docs, "unstable": unstable})
                continue
            function = self.parse_function()
            function["docs"] = docs
            function["unstable"] = unstable
            methods.append(function)
        return {"kind": "resource", "name": name, "methods": methods}

    def parse_function(self):
        name = self.identifier()
        self.expect(":")
        kind = "method"
        if self.accept("static"):
            kind = "static"
        self.expect("func")
        self.expect("(")
        parameters = self.parse_parameters()
        result = None
        if self.accept("->"):
            result = self.parse_type()
        self.expect(";")
        return {"name": name, "kind": kind, "parameters": parameters, "result": result}

    def parse_parameters(self):
        parameters = []
        while not self.accept(")"):
            parameter_name = self.identifier()
            self.expect(":")
            parameters.append((parameter_name, self.parse_type()))
            if not self.accept(","):
                self.expect(")")
                break
        return parameters


# ---------------------------------------------------------------- model

def kebab_to_pascal(name):
    return "".join(part[:1].upper() + part[1:] for part in name.split("-"))


def serde_kebab_of_pascal(pascal):
    """What serde's rename_all = "kebab-case" makes of a Rust variant name."""
    output = []
    for index, character in enumerate(pascal):
        if character.isupper():
            if index > 0:
                output.append("-")
            output.append(character.lower())
        else:
            output.append(character)
    return "".join(output)


def snake(name):
    return name.replace("-", "_")


def rust_value_name(name):
    """snake_case name, escaped when it is a Rust keyword."""
    candidate = snake(name)
    if candidate in NOT_RAW:
        return candidate + "_"
    if candidate in RUST_KEYWORDS:
        return "r#" + candidate
    return candidate


def rust_case_name(name):
    candidate = kebab_to_pascal(name)
    if candidate == "Self":
        return "Self_"
    return candidate


class Interface:
    """One parsed interface with its symbol table."""

    def __init__(self, directory, package, parsed, file_path, section):
        self.directory = directory
        self.package = package
        self.name = parsed["name"]
        self.docs = parsed["docs"]
        self.uses = parsed["uses"]
        self.types = parsed["types"]
        self.functions = parsed["functions"]
        self.file_path = file_path
        self.section = section
        self.defined = {definition["name"]: definition for definition in self.types}
        self.imported = {}

    @property
    def module(self):
        return rust_value_name(self.name)

    @property
    def wit_path(self):
        return f"{self.package}/{self.name}"


class Model:
    """All interfaces of the contract, with name resolution."""

    def __init__(self):
        self.interfaces = {}
        self.order = []
        self.worlds = []

    def add(self, interface):
        key = (interface.directory, interface.name)
        if key in self.interfaces:
            raise GenerateError(f"interface {interface.wit_path} defined twice")
        self.interfaces[key] = interface
        self.order.append(key)

    def directory_of_package(self, package_reference):
        name = package_reference.split("@", 1)[0]
        version = package_reference.split("@", 1)[1]
        if version != VERSION:
            raise GenerateError(f"use of {package_reference}: every package must be @{VERSION}")
        for directory, package in PACKAGE_OF_DIRECTORY.items():
            if package == name:
                return directory
        raise GenerateError(f"unknown package {package_reference}")

    def resolve_uses(self):
        for key in self.order:
            interface = self.interfaces[key]
            for use in interface.uses:
                directory = interface.directory if use["package"] is None else self.directory_of_package(use["package"])
                source = self.interfaces.get((directory, use["interface"]))
                if source is None:
                    raise GenerateError(f"{interface.wit_path}: use of unknown interface {directory}/{use['interface']}")
                for imported, alias in use["names"]:
                    interface.imported[alias] = (source, imported)
        for key in self.order:
            interface = self.interfaces[key]
            for alias in list(interface.imported):
                interface.imported[alias] = self.definition_site(interface, alias)

    def definition_site(self, interface, name, depth=0):
        """Follow use chains to the interface that defines `name`. Returns (interface, definition)."""
        if depth > 16:
            raise GenerateError(f"use chain too deep for {name}")
        if name in interface.defined:
            return interface, interface.defined[name]
        if name not in interface.imported:
            raise GenerateError(f"{interface.wit_path}: unknown type {name}")
        target = interface.imported[name]
        if isinstance(target[1], dict):
            return target
        source, imported = target
        return self.definition_site(source, imported, depth + 1)


# ---------------------------------------------------------------- loading

def wit_files(wit_directory):
    return sorted(path for path in wit_directory.rglob("*.wit") if path.is_file())


def wit_tree_sha256(wit_directory):
    """sha256 over every wit/**/*.wit file, sorted by relative POSIX path:
    for each file, the relative path, a NUL byte, the file bytes and a NUL byte."""
    digest = hashlib.sha256()
    for path in sorted(wit_files(wit_directory), key=lambda item: item.relative_to(wit_directory).as_posix()):
        digest.update(path.relative_to(wit_directory).as_posix().encode("utf-8"))
        digest.update(b"\0")
        digest.update(path.read_bytes())
        digest.update(b"\0")
    return digest.hexdigest()


def load_model(wit_directory):
    model = Model()
    section_counters = {directory: 0 for directory in PACKAGE_ORDER}
    for directory in PACKAGE_ORDER:
        package = PACKAGE_OF_DIRECTORY[directory]
        package_directory = wit_directory / directory
        if not package_directory.is_dir():
            raise GenerateError(f"missing directory {package_directory}")
        paths = sorted(package_directory.glob("*.wit"))
        parsed_files = []
        for path in paths:
            text = path.read_text(encoding="utf-8")
            parser = Parser(tokenize(text, str(path)), str(path))
            declared, items = parser.parse_file()
            if declared != f"{package}@{VERSION}":
                raise GenerateError(f"{path}: package line {declared!r}, expected {package}@{VERSION}")
            first_line = min((index for index, line in enumerate(text.split("\n")) if line.startswith("interface ")), default=0)
            parsed_files.append((path, items, first_line))
        # Declaration order: interfaces keep the order of the WIT reference. Per-interface
        # files carry no order of their own, so the order comes from EXTRACTED_FROM's page.
        for path, items, _ in parsed_files:
            for item in items:
                if item["kind"] != "interface":
                    raise GenerateError(f"{path}: worlds belong in wit/worlds/")
        ordered = order_by_page(wit_directory, directory, parsed_files)
        for path, item in ordered:
            section_counters[directory] += 1
            section = f"{SECTION_OF_PACKAGE[directory]}.{section_counters[directory]}"
            model.add(Interface(directory, package, item, path, section))
    for world_package in WORLD_PACKAGES:
        path = wit_directory / "worlds" / f"{world_package}.wit"
        text = path.read_text(encoding="utf-8")
        declared, items = Parser(tokenize(text, str(path)), str(path)).parse_file()
        if declared != f"ilium:{world_package}@{VERSION}":
            raise GenerateError(f"{path}: package line {declared!r}")
        for item in items:
            model.worlds.append((world_package, item))
    model.resolve_uses()
    return model


def order_by_page(wit_directory, directory, parsed_files):
    """Order interfaces as the WIT reference declares them, using the page named in EXTRACTED_FROM."""
    interfaces = []
    for path, items, _ in parsed_files:
        for item in items:
            interfaces.append((path, item))
    page_order = page_interface_order(wit_directory)
    package = PACKAGE_OF_DIRECTORY[directory]
    positions = page_order.get(package, [])

    def position(entry):
        name = entry[1]["name"]
        if name in positions:
            return positions.index(name)
        raise GenerateError(f"interface {package}/{name} is not in the WIT reference order")

    return sorted(interfaces, key=position)


_PAGE_ORDER_CACHE = {}


def page_interface_order(wit_directory):
    """Return {package: [interface names in page order]} from the files' own text when the
    page is not available. The types file holds its interfaces in page order already; the
    per-interface packages are ordered by the order list in wit/EXTRACTED_FROM's page."""
    key = str(wit_directory)
    if key in _PAGE_ORDER_CACHE:
        return _PAGE_ORDER_CACHE[key]
    order = {}
    types_text = (wit_directory / "types" / "ilium-types.wit").read_text(encoding="utf-8")
    order["ilium:types"] = re.findall(r"^interface ([a-z][a-z0-9-]*) \{", types_text, re.MULTILINE)
    # The world files list the imports in the order of the reference for host and
    # primitives only partially, so the canonical order is stored in INTERFACE_ORDER.
    for package, names in INTERFACE_ORDER.items():
        order[package] = list(names)
    _PAGE_ORDER_CACHE[key] = order
    return order


# Declaration order of the WIT reference (sections 5 to 8). The generator checks that
# this list names exactly the interface files found in wit/, so a new interface in the
# reference fails loudly until it is added here in its page position.
INTERFACE_ORDER = {
    "ilium:host": (
        "log", "clock", "timers", "journal", "tree", "panes", "intents-host", "storage", "settings",
        "codec", "fs", "http", "jobs", "process", "inference", "vector", "sounds", "topics",
        "commands", "services", "ui-host", "keys", "cli", "http-api", "workspace", "clipboard",
        "audio", "media", "location", "help", "plugins", "catalogs", "geocode", "sessions",
        "kernel-settings", "keymap", "gpu", "client-requests", "extensions",
    ),
    "ilium:primitives": ("terminal", "agent", "editor", "basic"),
    "ilium:anim": ("surface", "fonts", "renderer-types"),
    "ilium:exports": (
        "lifecycle", "handler", "views", "detector", "job-runner", "service-provider",
        "inference-provider", "annotator", "settings-validate", "catalog-provider", "menu-provider",
        "virtual-rows-provider", "token-provider", "hook-handler", "renderer", "migrator",
    ),
}


# ---------------------------------------------------------------- Rust rendering

class Renderer:
    """Turns the model into Rust source text."""

    def __init__(self, model):
        self.model = model
        self.default_cache = {}
        self.sampling = set()

    # -- names and paths

    def rust_type_name(self, definition):
        name = kebab_to_pascal(definition["name"])
        if definition["kind"] == "resource":
            name += "Handle"
        if name in RESERVED_TYPE_NAMES:
            raise GenerateError(f"WIT type {definition['name']} maps to the reserved Rust name {name}")
        return name

    def site(self, interface, name):
        return self.model.definition_site(interface, name)

    def type_reference(self, interface, wit_type, used):
        """Rust text for a WIT type used inside `interface`. Records cross-module names in `used`."""
        kind = wit_type[0]
        if kind == "unit":
            return "()"
        if kind == "primitive":
            return PRIMITIVES[wit_type[1]]
        if kind == "list":
            return f"Vec<{self.type_reference(interface, wit_type[1], used)}>"
        if kind == "option":
            return f"Option<{self.type_reference(interface, wit_type[1], used)}>"
        if kind in ("borrow", "own"):
            return self.type_reference(interface, wit_type[1], used)
        if kind == "tuple":
            members = ", ".join(self.type_reference(interface, member, used) for member in wit_type[1])
            return f"({members})"
        if kind == "result":
            ok = self.type_reference(interface, wit_type[1], used)
            error = self.type_reference(interface, wit_type[2], used)
            return f"Result<{ok}, {error}>"
        if kind == "named":
            source, definition = self.site(interface, wit_type[1])
            rust_name = self.rust_type_name(definition)
            if source is not interface:
                used.add((source.directory, source.module, rust_name))
            return rust_name
        raise GenerateError(f"unknown type kind {kind}")

    def module_path(self, interface, target_directory, target_module):
        if target_directory == interface.directory:
            return f"super::{target_module}"
        return f"super::super::{target_directory}::{target_module}"

    # -- trait derivability

    def has_default(self, interface, wit_type):
        kind = wit_type[0]
        if kind in ("unit", "primitive", "list", "option"):
            return True
        if kind == "tuple":
            return all(self.has_default(interface, member) for member in wit_type[1])
        if kind in ("borrow", "own", "result"):
            return False
        source, definition = self.site(interface, wit_type[1])
        return self.definition_has_default(source, definition)

    def definition_has_default(self, interface, definition):
        key = (interface.directory, interface.name, definition["name"])
        if key in self.default_cache:
            return self.default_cache[key]
        self.default_cache[key] = False
        kind = definition["kind"]
        if kind == "record":
            result = all(self.has_default(interface, field["type"]) for field in definition["fields"])
        elif kind == "flags":
            result = True
        elif kind == "type":
            result = self.has_default(interface, definition["target"])
        else:
            result = False
        self.default_cache[key] = result
        return result

    def is_id_newtype(self, interface, definition):
        return definition["kind"] == "type" and interface.directory == "types" and interface.name == "ids"

    # -- docs

    def doc_lines(self, docs, indent, location):
        lines = [f"{indent}/// {line}".rstrip() for line in docs]
        if docs:
            lines.append(f"{indent}///")
        lines.append(f"{indent}/// {location}")
        return lines

    def location(self, interface, item_name=None):
        suffix = f".{item_name}" if item_name else ""
        return f"WIT `{interface.wit_path}{suffix}`, reference-wit.md section {interface.section}."

    # -- items

    def render_interface(self, interface):
        used = set()
        body = []
        for definition in interface.types:
            body.append("")
            body.extend(self.render_definition(interface, definition, used))
        if interface.functions or any(definition["kind"] == "resource" and definition["methods"] for definition in interface.types):
            body.append("")
            body.extend(self.render_trait(interface, used))
        lines = []
        lines.extend(self.doc_lines(interface.docs, "", self.location(interface)))
        lines.append(f"pub mod {interface.module} {{")
        imports = {}
        for directory, module, rust_name in sorted(used):
            path = self.module_path(interface, directory, module)
            imports.setdefault(path, set()).add(rust_name)
        for path in sorted(imports):
            names = sorted(imports[path])
            if len(names) == 1:
                lines.append(f"    use {path}::{names[0]};")
            else:
                lines.extend(self.wrap_use(path, names))
        lines.extend(body if imports else body[1:])
        lines.append("}")
        return lines

    def wrap_use(self, path, names):
        single = f"    use {path}::{{{', '.join(names)}}};"
        if len(single) <= 100:
            return [single]
        lines = [f"    use {path}::{{"]
        current = "       "
        for name in names:
            piece = f" {name},"
            if len(current) + len(piece) > 100:
                lines.append(current)
                current = "       "
            current += piece
        lines.append(current)
        lines.append("    };")
        return lines

    def render_definition(self, interface, definition, used):
        kind = definition["kind"]
        location = self.location(interface, definition["name"])
        docs = self.doc_lines(definition["docs"], "    ", location)
        if definition.get("unstable"):
            docs.append(f"    /// Unstable: feature `{definition['unstable']}`.")
        if kind == "record":
            return docs + self.render_record(interface, definition, used)
        if kind == "enum":
            return docs + self.render_enum(definition)
        if kind == "variant":
            return docs + self.render_variant(interface, definition, used)
        if kind == "flags":
            return docs + self.render_flags(definition)
        if kind == "resource":
            return docs + self.render_resource(definition)
        if kind == "type":
            return docs + self.render_alias(interface, definition, used)
        raise GenerateError(f"unknown definition kind {kind}")

    def derives(self, interface, definition, extra=()):
        names = ["Debug", "Clone"]
        names.extend(extra)
        names.append("PartialEq")
        if definition["kind"] == "record" and self.definition_has_default(interface, definition):
            names.append("Default")
        names.extend(["serde::Serialize", "serde::Deserialize"])
        return f"    #[derive({', '.join(names)})]"

    def render_record(self, interface, definition, used):
        name = self.rust_type_name(definition)
        lines = [self.derives(interface, definition), '    #[serde(rename_all = "kebab-case")]']
        if not definition["fields"]:
            lines.append(f"    pub struct {name} {{}}")
            return lines
        lines.append(f"    pub struct {name} {{")
        for field in definition["fields"]:
            for doc in field["docs"]:
                lines.append(f"        /// {doc}".rstrip())
            field_name = rust_value_name(field["name"])
            if field_name.endswith("_") and not snake(field["name"]).endswith("_"):
                lines.append(f'        #[serde(rename = "{field["name"]}")]')
            rust_type = self.type_reference(interface, field["type"], used)
            lines.append(f"        pub {field_name}: {rust_type},")
        lines.append("    }")
        return lines

    def case_attributes(self, case_name, rust_name):
        if serde_kebab_of_pascal(rust_name.rstrip("_")) != case_name or rust_name.endswith("_"):
            return [f'        #[serde(rename = "{case_name}")]']
        return []

    def case_names_const(self, cases):
        names = [case["name"] for case in cases]
        single = f"        pub const CASE_NAMES: &'static [&'static str] = &[{', '.join(json.dumps(name) for name in names)}];"
        if len(single) <= 100:
            return [single]
        lines = ["        pub const CASE_NAMES: &'static [&'static str] = &["]
        for name in names:
            lines.append(f"            {json.dumps(name)},")
        lines.append("        ];")
        return lines

    def render_enum(self, definition):
        name = self.rust_type_name(definition)
        lines = [
            "    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]",
            '    #[serde(rename_all = "kebab-case")]',
            f"    pub enum {name} {{",
        ]
        for case in definition["cases"]:
            for doc in case["docs"]:
                lines.append(f"        /// {doc}".rstrip())
            if case.get("unstable"):
                lines.append(f"        /// Unstable: feature `{case['unstable']}`.")
            rust_case = rust_case_name(case["name"])
            lines.extend(self.case_attributes(case["name"], rust_case))
            lines.append(f"        {rust_case},")
        lines.append("    }")
        lines.append("")
        lines.append(f"    impl {name} {{")
        lines.append("        /// The WIT case names in declaration order (the ABI order).")
        lines.extend(self.case_names_const(definition["cases"]))
        lines.append("")
        lines.append("        /// The WIT name of this case.")
        lines.append("        pub fn as_str(self) -> &'static str {")
        lines.append("            match self {")
        for case in definition["cases"]:
            lines.append(f"                Self::{rust_case_name(case['name'])} => {json.dumps(case['name'])},")
        lines.append("            }")
        lines.append("        }")
        lines.append("    }")
        return lines

    def render_variant(self, interface, definition, used):
        name = self.rust_type_name(definition)
        lines = [
            "    #[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]",
            '    #[serde(rename_all = "kebab-case")]',
            f"    pub enum {name} {{",
        ]
        for case in definition["cases"]:
            for doc in case["docs"]:
                lines.append(f"        /// {doc}".rstrip())
            if case.get("unstable"):
                lines.append(f"        /// Unstable: feature `{case['unstable']}`.")
            rust_case = rust_case_name(case["name"])
            lines.extend(self.case_attributes(case["name"], rust_case))
            if case["payload"] is None:
                lines.append(f"        {rust_case},")
            else:
                lines.append(f"        {rust_case}({self.type_reference(interface, case['payload'], used)}),")
        lines.append("    }")
        lines.append("")
        lines.append(f"    impl {name} {{")
        lines.append("        /// The WIT case names in declaration order (the ABI order).")
        lines.extend(self.case_names_const(definition["cases"]))
        lines.append("")
        lines.append("        /// The WIT name of the case of this value.")
        lines.append("        pub fn case_name(&self) -> &'static str {")
        lines.append("            match self {")
        for case in definition["cases"]:
            pattern = f"Self::{rust_case_name(case['name'])}"
            if case["payload"] is not None:
                pattern += "(_)"
            lines.append(f"                {pattern} => {json.dumps(case['name'])},")
        lines.append("            }")
        lines.append("        }")
        lines.append("    }")
        return lines

    def render_flags(self, definition):
        name = self.rust_type_name(definition)
        cases = definition["cases"]
        if len(cases) > 32:
            raise GenerateError(f"flags {definition['name']} has more than 32 flags")
        lines = [
            "    #[derive(",
            "        Debug, Clone, Copy, PartialEq, Eq, Hash, Default, serde::Serialize, serde::Deserialize,",
            "    )]",
            "    #[serde(transparent)]",
            "    #[repr(transparent)]",
            f"    pub struct {name}(pub u32);",
            "",
            f"    impl {name} {{",
        ]
        for bit, case in enumerate(cases):
            for doc in case["docs"]:
                lines.append(f"        /// {doc}".rstrip())
            lines.append(f"        /// Flag `{case['name']}` (bit {bit}).")
            lines.append(f"        pub const {snake(case['name']).upper()}: {name} = {name}(1 << {bit});")
        lines.extend([
            "        /// The WIT flag names in declaration order; flag `n` is bit `n`.",
        ])
        lines.extend(self.case_names_const(cases))
        all_bits = (1 << len(cases)) - 1
        lines.extend([
            f"        /// Every flag of `{definition['name']}` set.",
            f"        pub const ALL: {name} = {name}({all_bits:#x});",
            "",
            "        /// No flag set.",
            f"        pub const fn empty() -> {name} {{",
            f"            {name}(0)",
            "        }",
            "",
            "        /// The raw bits.",
            "        pub const fn bits(self) -> u32 {",
            "            self.0",
            "        }",
            "",
            "        /// True when every flag of `other` is set in `self`.",
            f"        pub const fn contains(self, other: {name}) -> bool {{",
            "            self.0 & other.0 == other.0",
            "        }",
            "",
            "        /// True when no flag is set.",
            "        pub const fn is_empty(self) -> bool {",
            "            self.0 == 0",
            "        }",
            "",
            "        /// Sets every flag of `other`.",
            f"        pub fn insert(&mut self, other: {name}) {{",
            "            self.0 |= other.0;",
            "        }",
            "",
            "        /// Clears every flag of `other`.",
            f"        pub fn remove(&mut self, other: {name}) {{",
            "            self.0 &= !other.0;",
            "        }",
            "    }",
            "",
            f"    impl std::ops::BitOr for {name} {{",
            f"        type Output = {name};",
            "",
            f"        fn bitor(self, other: {name}) -> {name} {{",
            f"            {name}(self.0 | other.0)",
            "        }",
            "    }",
            "",
            f"    impl std::ops::BitAnd for {name} {{",
            f"        type Output = {name};",
            "",
            f"        fn bitand(self, other: {name}) -> {name} {{",
            f"            {name}(self.0 & other.0)",
            "        }",
            "    }",
            "",
            f"    impl std::ops::BitOrAssign for {name} {{",
            f"        fn bitor_assign(&mut self, other: {name}) {{",
            "            self.0 |= other.0;",
            "        }",
            "    }",
        ])
        return lines

    def render_resource(self, definition):
        name = self.rust_type_name(definition)
        return [
            "    /// Opaque handle of the resource; its methods are methods of this interface's trait.",
            "    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]",
            "    #[serde(transparent)]",
            "    #[repr(transparent)]",
            f"    pub struct {name}(pub u32);",
        ]

    def render_alias(self, interface, definition, used):
        name = self.rust_type_name(definition)
        if self.is_id_newtype(interface, definition):
            target = definition["target"]
            if target[0] != "primitive":
                raise GenerateError(f"id alias {definition['name']} is not a primitive")
            rust_target = PRIMITIVES[target[1]]
            copy = "" if rust_target == "String" else "Copy, "
            derive_line = (
                f"    #[derive(Debug, Clone, {copy}PartialEq, Eq, Hash, PartialOrd, Ord, Default, "
                "serde::Serialize, serde::Deserialize)]"
            )
            if len(derive_line) > 100:
                derive_lines = [
                    "    #[derive(",
                    "        Debug,",
                    "        Clone,",
                ]
                if copy:
                    derive_lines.append("        Copy,")
                derive_lines.extend([
                    "        PartialEq,",
                    "        Eq,",
                    "        Hash,",
                    "        PartialOrd,",
                    "        Ord,",
                    "        Default,",
                    "        serde::Serialize,",
                    "        serde::Deserialize,",
                    "    )]",
                ])
            else:
                derive_lines = [derive_line]
            return derive_lines + [
                "    #[serde(transparent)]",
                "    #[repr(transparent)]",
                f"    pub struct {name}(pub {rust_target});",
            ]
        return [f"    pub type {name} = {self.type_reference(interface, definition['target'], used)};"]

    def trait_name(self, interface):
        prefix = "Export" if interface.directory == "exports" else "Host"
        return prefix + kebab_to_pascal(interface.name)

    def render_trait(self, interface, used):
        trait = self.trait_name(interface)
        implementer = "the guest" if interface.directory == "exports" else "the host"
        lines = [
            f"    /// Functions of `{interface.wit_path}`, implemented by {implementer}.",
            f"    /// {self.location(interface)}",
            f"    pub trait {trait} {{",
        ]
        methods = []
        for function in interface.functions:
            methods.append((function, None))
        for definition in interface.types:
            if definition["kind"] == "resource":
                for method in definition["methods"]:
                    methods.append((method, definition))
        first = True
        for function, resource in methods:
            if not first:
                lines.append("")
            first = False
            for doc in function.get("docs", []):
                lines.append(f"        /// {doc}".rstrip())
            if function.get("unstable"):
                lines.append(f"        /// Unstable: feature `{function['unstable']}`.")
            parameters = ["&mut self"]
            if resource is None:
                method_name = rust_value_name(function["name"])
                wit_name = function["name"]
            else:
                handle = self.rust_type_name(resource)
                if function["kind"] == "constructor":
                    method_name = rust_value_name(f"{resource['name']}-new")
                    wit_name = f"[constructor]{resource['name']}"
                else:
                    method_name = rust_value_name(f"{resource['name']}-{function['name']}")
                    wit_name = f"[method]{resource['name']}.{function['name']}"
                    if function["kind"] != "static":
                        parameters.append(f"handle: {handle}")
            lines.append(f"        /// WIT `{wit_name}`.")
            for parameter_name, parameter_type in function["parameters"]:
                parameters.append(f"{rust_value_name(parameter_name)}: {self.type_reference(interface, parameter_type, used)}")
            if resource is not None and function["kind"] == "constructor":
                result = f" -> {self.rust_type_name(resource)}"
            elif function["result"] is None:
                result = ""
            else:
                result = f" -> {self.type_reference(interface, function['result'], used)}"
            lines.extend(self.format_signature(method_name, parameters, result))
        lines.append("    }")
        return lines

    def format_signature(self, method_name, parameters, result):
        single = f"        fn {method_name}({', '.join(parameters)}){result};"
        if len(single) <= 100:
            return [single]
        lines = [f"        fn {method_name}("]
        for parameter in parameters:
            lines.append(f"            {parameter},")
        closing = f"        ){result};"
        if len(closing) > 100:
            raise GenerateError(f"return type of {method_name} is too long to format")
        lines.append(closing)
        return lines

    # -- package files

    def package_file(self, directory, header_lines):
        interfaces = [self.model.interfaces[key] for key in self.model.order if key[0] == directory]
        lines = list(header_lines)
        lines.append(f"//! Package `{PACKAGE_OF_DIRECTORY[directory]}@{VERSION}`: one module per interface.")
        lines.append("//! Generated by tools/gen_plugin_api.py from wit/. Never edit by hand.")
        lines.append("//!")
        lines.append("//! The allowed lints are inherent to a faithful mirror of the WIT: variant sizes, parameter")
        lines.append("//! counts and nested types follow the contract and cannot change without breaking the ABI.")
        lines.append("#![allow(")
        for lint in CLIPPY_ALLOW:
            lines.append(f"    {lint},")
        lines[-1] = lines[-1].rstrip(",")
        lines.append(")]")
        exported = {}
        for interface in interfaces:
            names = [self.rust_type_name(definition) for definition in interface.types]
            if interface.functions or any(definition["kind"] == "resource" and definition["methods"] for definition in interface.types):
                names.append(self.trait_name(interface))
            for name in names:
                exported.setdefault(name, []).append(interface)
        unique = {}
        duplicated = []
        for name, owners in exported.items():
            if len(owners) == 1:
                unique.setdefault(owners[0].module, []).append(name)
            else:
                duplicated.append((name, owners))
        if unique:
            lines.append("")
            for interface in sorted(interfaces, key=lambda item: item.module):
                names = sorted(unique.get(interface.module, []))
                if not names:
                    continue
                single = f"pub use {interface.module}::{{{', '.join(names)}}};" if len(names) > 1 else f"pub use {interface.module}::{names[0]};"
                if len(single) <= 100:
                    lines.append(single)
                else:
                    lines.append(f"pub use {interface.module}::{{")
                    current = "   "
                    for name in names:
                        piece = f" {name},"
                        if len(current) + len(piece) > 100:
                            lines.append(current)
                            current = "   "
                        current += piece
                    lines.append(current)
                    lines.append("};")
        if duplicated:
            lines.append("")
            lines.append("// Defined in more than one interface of this package, so not re-exported:")
            for name, owners in sorted(duplicated, key=lambda item: item[0]):
                lines.append(f"// {name}: {', '.join(owner.module for owner in owners)}")
        for interface in interfaces:
            lines.append("")
            lines.extend(self.render_interface(interface))
        return "\n".join(lines) + "\n", duplicated

    def mod_file(self, header_lines, tree_sha256):
        lines = list(header_lines)
        lines.append("//! Rust mirror types of the WIT contract `ilium:plugin@0.1.0` (reference-wit.md).")
        lines.append("//! Generated by tools/gen_plugin_api.py from wit/. Never edit by hand.")
        lines.append("")
        for directory in ("anim", "exports", "host", "primitives", "types"):
            lines.append(f"pub mod {directory};")
        lines.append("")
        lines.append("/// sha256 of the wit/ tree this code was generated from (see tools/gen_plugin_api.py).")
        lines.append(f'pub const WIT_SHA256: &str = "{tree_sha256}";')
        lines.append("")
        lines.append("/// The seven WIT packages of the contract, all at the one version (ABI-003).")
        lines.append("pub const PACKAGES: &[&str] = &[")
        for package in ("ilium:types", "ilium:host", "ilium:primitives", "ilium:anim", "ilium:exports",
                        "ilium:server", "ilium:client"):
            lines.append(f'    "{package}@{VERSION}",')
        lines.append("];")
        lines.append("")
        lines.append("/// One WIT function or resource method of the contract.")
        lines.append("#[derive(Debug, Clone, Copy, PartialEq, Eq)]")
        lines.append("pub struct WitFunction {")
        lines.append("    /// The package, for example `ilium:host`.")
        lines.append("    pub package: &'static str,")
        lines.append("    /// The interface, for example `http`.")
        lines.append("    pub interface: &'static str,")
        lines.append("    /// The WIT name; resource members are `[constructor]r`, `[method]r.m`.")
        lines.append("    pub name: &'static str,")
        lines.append("    /// The WIT text of the result type, empty when the function returns nothing.")
        lines.append("    pub result: &'static str,")
        lines.append("    /// The unstable feature that gates the function, if any.")
        lines.append("    pub unstable: Option<&'static str>,")
        lines.append("}")
        lines.append("")
        lines.append("/// Every function of `ilium:host`, `ilium:primitives`, `ilium:anim` and `ilium:exports`,")
        lines.append("/// in declaration order.")
        lines.append("pub const FUNCTIONS: &[WitFunction] = &[")
        for key in self.model.order:
            interface = self.model.interfaces[key]
            entries = [(function, None) for function in interface.functions]
            for definition in interface.types:
                if definition["kind"] == "resource":
                    entries.extend((method, definition) for method in definition["methods"])
            for function, resource in entries:
                if resource is None:
                    name = function["name"]
                elif function["kind"] == "constructor":
                    name = f"[constructor]{resource['name']}"
                else:
                    name = f"[method]{resource['name']}.{function['name']}"
                result = wit_type_text(function["result"]) if function.get("result") else ""
                if resource is not None and function["kind"] == "constructor":
                    result = resource["name"]
                unstable = f'Some("{function["unstable"]}")' if function.get("unstable") else "None"
                lines.append("    WitFunction {")
                lines.append(f'        package: "{interface.package}",')
                lines.append(f'        interface: "{interface.name}",')
                lines.append(f'        name: "{name}",')
                lines.append(f"        result: {json.dumps(result)},")
                lines.append(f"        unstable: {unstable},")
                lines.append("    },")
        lines.append("];")
        lines.append("")
        lines.append("/// Every item marked `// @unstable(feature = ...)`: (feature, WIT path of the item).")
        lines.append("pub const UNSTABLE_ITEMS: &[(&str, &str)] = &[")
        for feature, path in self.unstable_items():
            lines.append(f'    ("{feature}", "{path}"),')
        lines.append("];")
        lines.append("")
        lines.append("/// The worlds of the contract: (world package, world name, WIT paths it imports and exports).")
        lines.append("pub const WORLDS: &[(&str, &str, &[&str], &[&str])] = &[")
        for world_package, world in self.model.worlds:
            imports = [f"{entry['package'].split('@')[0]}/{entry['interface']}" for entry in world["entries"] if entry["kind"] == "import"]
            exports = [f"{entry['package'].split('@')[0]}/{entry['interface']}" for entry in world["entries"] if entry["kind"] == "export"]
            lines.append("    (")
            lines.append(f'        "ilium:{world_package}",')
            lines.append(f'        "{world["name"]}",')
            lines.extend(self.string_slice(imports, "        "))
            lines.extend(self.string_slice(exports, "        "))
            lines.append("    ),")
        lines.append("];")
        return "\n".join(lines) + "\n"

    def string_slice(self, values, indent):
        if not values:
            return [f"{indent}&[],"]
        single = f"{indent}&[{', '.join(json.dumps(value) for value in values)}],"
        if len(single) <= 100:
            return [single]
        lines = [f"{indent}&["]
        for value in values:
            lines.append(f"{indent}    {json.dumps(value)},")
        lines.append(f"{indent}],")
        return lines

    def unstable_items(self):
        items = []
        for key in self.model.order:
            interface = self.model.interfaces[key]
            for definition in interface.types:
                if definition.get("unstable"):
                    items.append((definition["unstable"], f"{interface.wit_path}.{definition['name']}"))
                for case in definition.get("cases", []):
                    if case.get("unstable"):
                        items.append((case["unstable"], f"{interface.wit_path}.{definition['name']}.{case['name']}"))
                for method in definition.get("methods", []):
                    if method.get("unstable"):
                        items.append((method["unstable"], f"{interface.wit_path}.{definition['name']}.{method['name']}"))
            for function in interface.functions:
                if function.get("unstable"):
                    items.append((function["unstable"], f"{interface.wit_path}.{function['name']}"))
        return items

    # -- serde round trip test

    def sample(self, interface, wit_type, depth=0):
        """A Rust expression with a non-default value of `wit_type`, usable from the test crate."""
        if depth > 64:
            raise GenerateError("sample nesting too deep")
        kind = wit_type[0]
        if kind == "unit":
            return "()"
        if kind == "primitive":
            return {
                "u8": "7u8", "u16": "7u16", "u32": "7u32", "u64": "7u64",
                "s8": "-7i8", "s16": "-7i16", "s32": "-7i32", "s64": "-7i64",
                "f32": "1.5f32", "f64": "1.5f64", "bool": "true", "char": "'x'",
                "string": 'String::from("x")',
            }[wit_type[1]]
        if kind == "list":
            if self.is_active(interface, wit_type[1]):
                return "Vec::new()"
            return f"vec![{self.sample(interface, wit_type[1], depth + 1)}]"
        if kind == "option":
            if self.is_active(interface, wit_type[1]):
                return "None"
            return f"Some({self.sample(interface, wit_type[1], depth + 1)})"
        if kind in ("borrow", "own"):
            return self.sample(interface, wit_type[1], depth + 1)
        if kind == "tuple":
            members = [self.sample(interface, member, depth + 1) for member in wit_type[1]]
            return f"({', '.join(members)},)" if len(members) == 1 else f"({', '.join(members)})"
        if kind == "result":
            return f"Ok({self.sample(interface, wit_type[1], depth + 1)})"
        source, definition = self.site(interface, wit_type[1])
        return self.definition_sample(source, definition, depth + 1)

    def is_active(self, interface, wit_type):
        """True when `wit_type` names a type whose sample is being built (a recursive type)."""
        if wit_type[0] != "named":
            return False
        source, definition = self.site(interface, wit_type[1])
        return (source.wit_path, definition["name"]) in self.sampling

    def full_path(self, interface, definition):
        return f"generated::{interface.directory}::{interface.module}::{self.rust_type_name(definition)}"

    def definition_sample(self, interface, definition, depth, case=None):
        key = (interface.wit_path, definition["name"])
        self.sampling.add(key)
        try:
            return self.definition_sample_inner(interface, definition, depth, case)
        finally:
            self.sampling.discard(key)

    def definition_sample_inner(self, interface, definition, depth, case):
        path = self.full_path(interface, definition)
        kind = definition["kind"]
        if kind == "record":
            fields = ", ".join(
                f"{rust_value_name(field['name'])}: {self.sample(interface, field['type'], depth)}" for field in definition["fields"]
            )
            return f"{path} {{ {fields} }}" if fields else f"{path} {{}}"
        if kind == "enum":
            chosen = case or definition["cases"][-1]
            return f"{path}::{rust_case_name(chosen['name'])}"
        if kind == "variant":
            chosen = case or definition["cases"][0]
            if chosen["payload"] is None:
                return f"{path}::{rust_case_name(chosen['name'])}"
            return f"{path}::{rust_case_name(chosen['name'])}({self.sample(interface, chosen['payload'], depth)})"
        if kind == "flags":
            bits = (1 << len(definition["cases"])) - 1
            return f"{path}({bits:#x})"
        if kind == "resource":
            return f"{path}(3)"
        if kind == "type":
            if self.is_id_newtype(interface, definition):
                target = definition["target"]
                return f"{path}({self.sample(interface, target, depth)})"
            return self.sample(interface, definition["target"], depth)
        raise GenerateError(f"no sample for {kind}")

    def roundtrip_file(self, header_lines):
        lines = list(header_lines)
        lines.append("//! Serde round trip of every generated type: every record, every enum case, every")
        lines.append("//! variant case, every flags type, id newtype and resource handle. Generated by")
        lines.append("//! tools/gen_plugin_api.py; never edit by hand. Pure data: these tests pass against the skeleton.")
        lines.append("")
        lines.append("use ilium_plugin_api::generated;")
        lines.append("")
        lines.append("fn round_trip<T>(value: T)")
        lines.append("where")
        lines.append("    T: serde::Serialize + serde::de::DeserializeOwned + PartialEq + std::fmt::Debug,")
        lines.append("{")
        lines.append('    let json = serde_json::to_string(&value).expect("serialize");')
        lines.append('    let back: T = serde_json::from_str(&json).expect("deserialize");')
        lines.append('    assert_eq!(back, value, "round trip through {json}");')
        lines.append("}")
        count = 0
        for key in self.model.order:
            interface = self.model.interfaces[key]
            samples = []
            for definition in interface.types:
                kind = definition["kind"]
                if kind == "type" and not self.is_id_newtype(interface, definition):
                    continue
                if kind in ("enum", "variant"):
                    for case in definition["cases"]:
                        samples.append(self.definition_sample(interface, definition, 0, case))
                else:
                    samples.append(self.definition_sample(interface, definition, 0))
            if not samples:
                continue
            count += 1
            test_name = f"abi_020_serde_round_trip_{interface.directory}_{snake(interface.name)}"
            lines.append("")
            lines.append("/// REQ: ABI-020, ABI-016")
            lines.append("#[test]")
            lines.append(f"fn {test_name}() {{")
            for sample in samples:
                lines.extend(format_call(f"round_trip({sample});", "    "))
            lines.append("}")
        return "\n".join(lines) + "\n", count


def format_call(statement, indent):
    """One statement line; rustfmt wraps it later."""
    return [indent + statement]


def wit_type_text(wit_type):
    kind = wit_type[0]
    if kind == "unit":
        return "_"
    if kind == "primitive":
        return wit_type[1]
    if kind in ("list", "option", "borrow", "own"):
        return f"{kind}<{wit_type_text(wit_type[1])}>"
    if kind == "tuple":
        return f"tuple<{', '.join(wit_type_text(member) for member in wit_type[1])}>"
    if kind == "result":
        ok = wit_type_text(wit_type[1])
        error = wit_type_text(wit_type[2])
        if error == "_":
            return f"result<{ok}>"
        return f"result<{ok}, {error}>"
    return wit_type[1]


# ---------------------------------------------------------------- driver

def rustfmt(text, label):
    """Format Rust source with the toolchain's rustfmt (parse and print only, no compilation).

    The generator emits readable but unformatted text; rustfmt makes it match `cargo fmt`, so the
    workspace's `cargo fmt --check` stays clean. The result depends on the rustfmt version, which
    is the workspace toolchain's (rust-toolchain / CI); --check reports drift after an upgrade."""
    binary = shutil.which("rustfmt")
    if binary is None:
        raise GenerateError("rustfmt not found on PATH; it formats the generated code")
    completed = subprocess.run(
        [binary, "--edition", "2021", "--emit", "stdout", "--quiet"],
        input=text.encode("utf-8"),
        capture_output=True,
        check=False,
    )
    if completed.returncode != 0:
        raise GenerateError(f"rustfmt failed on {label}: {completed.stderr.decode('utf-8', 'replace')[:2000]}")
    return completed.stdout.decode("utf-8")


def header(tree_sha256, output_path):
    return [f"// @generated from wit/ sha256={tree_sha256}", f"// output: {output_path}"]


def build_outputs(root):
    wit_directory = root / "wit"
    if not wit_directory.is_dir():
        raise GenerateError(f"missing {wit_directory}; run tools/extract_wit.py first")
    tree_sha256 = wit_tree_sha256(wit_directory)
    model = load_model(wit_directory)
    check_interface_order(model, wit_directory)
    renderer = Renderer(model)
    outputs = {}
    duplicates = {}
    for crate in CRATES:
        mod_path = f"{crate}/src/generated/mod.rs"
        outputs[mod_path] = rustfmt(renderer.mod_file(header(tree_sha256, mod_path), tree_sha256), mod_path)
        for directory in PACKAGE_ORDER:
            path = f"{crate}/src/generated/{directory}.rs"
            text, duplicated = renderer.package_file(directory, header(tree_sha256, path))
            outputs[path] = rustfmt(text, path)
            duplicates[directory] = [name for name, _ in duplicated]
    roundtrip_path = f"{ROUNDTRIP_CRATE}/tests/abi/generated_roundtrip.rs"
    roundtrip_text, roundtrip_tests = renderer.roundtrip_file(header(tree_sha256, roundtrip_path))
    outputs[roundtrip_path] = rustfmt(roundtrip_text, roundtrip_path)
    golden_lines = []
    for directory in ("mod",) + PACKAGE_ORDER:
        relative = f"src/generated/{directory}.rs"
        data = outputs[f"{ROUNDTRIP_CRATE}/{relative}"].encode("utf-8")
        golden_lines.append(f"{hashlib.sha256(data).hexdigest()}  {relative}")
    outputs[f"{ROUNDTRIP_CRATE}/tests/abi/golden/generated.sha256"] = "\n".join(golden_lines) + "\n"
    statistics = statistics_of(model)
    statistics["roundtrip_tests"] = roundtrip_tests
    statistics["duplicates"] = duplicates
    return tree_sha256, outputs, statistics


def check_interface_order(model, wit_directory):
    for package, names in INTERFACE_ORDER.items():
        directory = [key for key, value in PACKAGE_OF_DIRECTORY.items() if value == package][0]
        found = sorted(path.stem for path in (wit_directory / directory).glob("*.wit"))
        if sorted(names) != found:
            raise GenerateError(f"INTERFACE_ORDER for {package} does not match the files in wit/{directory}/")


def statistics_of(model):
    counts = {"interfaces": len(model.order), "records": 0, "enums": 0, "variants": 0, "flags": 0,
              "resources": 0, "aliases": 0, "functions": 0, "worlds": len(model.worlds)}
    for key in model.order:
        interface = model.interfaces[key]
        counts["functions"] += len(interface.functions)
        for definition in interface.types:
            kind = definition["kind"]
            plural = {"record": "records", "enum": "enums", "variant": "variants", "flags": "flags",
                      "resource": "resources", "type": "aliases"}[kind]
            counts[plural] += 1
            if kind == "resource":
                counts["functions"] += len(definition["methods"])
    return counts


def main(argv):
    parser = argparse.ArgumentParser(description=__doc__.split("\n")[0])
    default_root = pathlib.Path(__file__).resolve().parent.parent
    parser.add_argument("--root", type=pathlib.Path, default=default_root, help="workspace root (default: parent of tools/)")
    parser.add_argument("--check", action="store_true", help="compare with the files on disk instead of writing; exit 1 on drift")
    try:
        arguments = parser.parse_args(argv)
    except SystemExit as exit_request:
        return 2 if exit_request.code else 0
    root = arguments.root.resolve()
    try:
        tree_sha256, outputs, statistics = build_outputs(root)
    except GenerateError as error:
        emit({"type": "error", "code": "generate", "message": str(error)})
        return 2
    drift = []
    for relative, text in outputs.items():
        target = root / relative
        data = text.encode("utf-8")
        current = target.read_bytes() if target.is_file() else None
        digest = hashlib.sha256(data).hexdigest()
        if current == data:
            emit({"type": "artifact", "path": str(target), "status": "unchanged", "sha256": digest})
            continue
        drift.append(relative)
        if arguments.check:
            emit({"type": "warning", "code": "drift", "path": str(target)})
            continue
        target.parent.mkdir(parents=True, exist_ok=True)
        target.write_bytes(data)
        emit({"type": "artifact", "path": str(target), "status": "written", "sha256": digest})
    summary = {"type": "summary", "wit_sha256": tree_sha256, "files": len(outputs), "drift": len(drift),
               "mode": "check" if arguments.check else "write"}
    summary.update(statistics)
    emit(summary)
    if arguments.check and drift:
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
