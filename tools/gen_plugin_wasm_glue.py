"""wasm32 guest glue of the plugin SDK, generated from the same WIT model as the mirror types.

Called by tools/gen_plugin_api.py (build_outputs); not a command of its own.

Outputs:

    ilium-plugin-sdk-core/src/wasm/glue.rs
        * `bindings`: one wit_bindgen::generate! over every interface of ilium:types,
          ilium:host, ilium:primitives and ilium:anim (imports only, no exports).
        * conversions between the wit-bindgen types and the serde mirror types
          (`IntoWit` mirror -> bindgen, `IntoMirror` bindgen -> mirror).
        * `imports`: one free function per host function of the interfaces named by
          `HostCalls` (ilium-plugin-sdk-core/src/plugin.rs); it converts, calls the bindgen
          import and converts back. A function nobody calls is dropped at link time and
          its import with it.
        * `__ilium_wasm_host_<interface>!`: implements the mirror `Host*` trait for the
          plugin's host type, either on the imports (`on`) or on the detached handle (`off`).
        * `__ilium_wasm_export_<interface>!`: implements the wit-bindgen `Guest` trait of
          one export interface by forwarding to the mirror `Export*` trait of the plugin.

    ilium-plugin-sdk-macros/src/generated_glue.rs
        the tables the `#[ilium_plugin]` macro needs: export interfaces with the exact
        `with:` remapping of each, and the host interfaces with their grant names.

Naming follows wit-bindgen 0.57.1 (to_rust_ident, to_upper_camel_case, parameter modes of
`Ownership::Owning`); the rules are restated next to the code that depends on them.
"""

import re

# wit-bindgen-rust 0.57.1 `to_rust_ident`: these names get a trailing underscore.
BINDGEN_KEYWORDS = {
    "as", "break", "const", "continue", "crate", "else", "enum", "extern", "false", "fn", "for",
    "if", "impl", "in", "let", "loop", "match", "mod", "move", "mut", "pub", "ref", "return",
    "self", "static", "struct", "super", "trait", "true", "type", "unsafe", "use", "where",
    "while", "async", "await", "dyn", "abstract", "become", "box", "do", "final", "macro",
    "override", "priv", "typeof", "unsized", "virtual", "yield", "try",
}

# Packages whose interfaces are imports of a plugin world.
IMPORT_DIRECTORIES = ("types", "host", "primitives", "anim")
# Export interfaces without glue yet: they export a resource (wave two, with the scene side).
EXPORTS_WITHOUT_GLUE = ("renderer",)
# Side name of the macro -> world package in wit/worlds/.
WORLD_OF_SIDE = (("server", "server"), ("client", "client"), ("scene", "anim"))
# Imports the host links without any grant (ilium-plugin-host/src/engine.rs import_allowed).
ALWAYS_ALLOWED = (
    "log", "telemetry", "self-limits", "self-describe", "clock", "timers", "storage", "settings",
    "codec", "messages",
)

SDK_CORE = "ilium-plugin-sdk-core"
SDK_MACROS = "ilium-plugin-sdk-macros"
GLUE_PATH = f"{SDK_CORE}/src/wasm/glue.rs"
TABLE_PATH = f"{SDK_MACROS}/src/generated_glue.rs"


class Context:
    """Where the generated text lives: paths differ inside sdk-core and inside a macro."""

    def __init__(self, bindings_root, local_bindings, mirror_root, traits):
        self.bindings_root = bindings_root
        self.local_bindings = local_bindings
        self.mirror_root = mirror_root
        self.traits = traits


IN_SDK = Context("b", "b", "m", "")
IN_MACRO = Context("$crate::wasm::bindings", "bindings", "$crate::generated", "$crate::wasm::")


class Glue:
    def __init__(self, api, model, renderer, root):
        self.api = api
        self.model = model
        self.renderer = renderer
        self.root = root
        self.list_cache = {}
        self.handle_cache = {}

    def fail(self, message):
        raise self.api.GenerateError(f"wasm glue: {message}")

    # -- names

    def bindgen_ident(self, name):
        if name in BINDGEN_KEYWORDS:
            return name + "_"
        return self.api.snake(name)

    def bindgen_camel(self, name):
        if name == "guest":
            return "Guest_"
        return self.api.kebab_to_pascal(name)

    def package_name(self, interface):
        return self.api.PACKAGE_OF_DIRECTORY[interface.directory].split(":", 1)[1]

    def wit_id(self, interface):
        return f"{self.api.PACKAGE_OF_DIRECTORY[interface.directory]}/{interface.name}@{self.api.VERSION}"

    def bindgen_module_suffix(self, interface):
        return f"ilium::{self.bindgen_ident(self.package_name(interface))}::{self.bindgen_ident(interface.name)}"

    def bindgen_module(self, interface, context):
        suffix = self.bindgen_module_suffix(interface)
        if interface.directory == "exports":
            return f"{context.local_bindings}::exports::{suffix}"
        return f"{context.bindings_root}::{suffix}"

    def bindgen_name(self, interface, definition, context):
        return f"{self.bindgen_module(interface, context)}::{self.bindgen_camel(definition['name'])}"

    def mirror_module(self, interface, context):
        return f"{context.mirror_root}::{interface.directory}::{interface.module}"

    def mirror_name(self, interface, definition, context):
        return f"{self.mirror_module(interface, context)}::{self.renderer.rust_type_name(definition)}"

    def mirror_trait(self, interface, context):
        return f"{self.mirror_module(interface, context)}::{self.renderer.trait_name(interface)}"

    # -- types

    def type_text(self, interface, wit_type, context, mirror):
        kind = wit_type[0]
        if kind == "unit":
            return "()"
        if kind == "primitive":
            name = self.api.PRIMITIVES[wit_type[1]]
            return "::std::string::String" if name == "String" else name
        if kind == "list":
            return f"::std::vec::Vec<{self.type_text(interface, wit_type[1], context, mirror)}>"
        if kind == "option":
            return f"::core::option::Option<{self.type_text(interface, wit_type[1], context, mirror)}>"
        if kind in ("own", "borrow"):
            return self.type_text(interface, wit_type[1], context, mirror)
        if kind == "tuple":
            members = "".join(f"{self.type_text(interface, member, context, mirror)}, " for member in wit_type[1])
            return f"({members})"
        if kind == "result":
            ok = self.type_text(interface, wit_type[1], context, mirror)
            error = self.type_text(interface, wit_type[2], context, mirror)
            return f"::core::result::Result<{ok}, {error}>"
        if kind == "named":
            source, definition = self.model.definition_site(interface, wit_type[1])
            if mirror:
                return self.mirror_name(source, definition, context)
            return self.bindgen_name(source, definition, context)
        self.fail(f"unknown type kind {kind}")
        return None

    def scan(self, interface, wit_type, cache, leaf):
        """True when `leaf(kind, payload)` holds anywhere inside the type."""
        kind = wit_type[0]
        if leaf(wit_type):
            return True
        if kind in ("unit", "primitive"):
            return False
        if kind in ("list", "option", "own", "borrow"):
            return self.scan(interface, wit_type[1], cache, leaf)
        if kind == "tuple":
            return any(self.scan(interface, member, cache, leaf) for member in wit_type[1])
        if kind == "result":
            return self.scan(interface, wit_type[1], cache, leaf) or self.scan(interface, wit_type[2], cache, leaf)
        source, definition = self.model.definition_site(interface, wit_type[1])
        key = (source.directory, source.name, definition["name"])
        if key in cache:
            return cache[key]
        cache[key] = False
        definition_kind = definition["kind"]
        if definition_kind == "record":
            found = any(self.scan(source, field["type"], cache, leaf) for field in definition["fields"])
        elif definition_kind == "variant":
            found = any(
                case["payload"] is not None and self.scan(source, case["payload"], cache, leaf)
                for case in definition["cases"]
            )
        elif definition_kind == "type":
            found = self.scan(source, definition["target"], cache, leaf)
        elif definition_kind == "resource":
            found = leaf(("resource",))
        else:
            found = False
        cache[key] = found
        return found

    def has_list(self, interface, wit_type):
        """wit-bindgen `TypeInfo::has_list`: a list or a string anywhere inside."""
        return self.scan(
            interface, wit_type, self.list_cache,
            lambda item: item[0] == "list" or item == ("primitive", "string"),
        )

    def has_handle(self, interface, wit_type):
        return self.scan(
            interface, wit_type, self.handle_cache,
            lambda item: item[0] in ("own", "borrow", "resource"),
        )

    def dealias(self, interface, wit_type):
        """Follow `type a = b` aliases; returns (interface, type) of the first non-alias."""
        while wit_type[0] == "named":
            source, definition = self.model.definition_site(interface, wit_type[1])
            if definition["kind"] != "type":
                return interface, wit_type
            interface, wit_type = source, definition["target"]
        return interface, wit_type

    def is_string_or_list(self, interface, wit_type):
        _, core = self.dealias(interface, wit_type)
        return core == ("primitive", "string") or core[0] == "list"

    # -- import arguments
    #
    # wit-bindgen prints the parameters of an imported function with
    # `TypeOwnershipStyle::OnlyTopBorrowed` (interface.rs type_mode_for_id, print_ty,
    # AnonTypeGenerator). A type with no list or string inside is passed by value. Otherwise:
    # string -> &str, list<T> -> &[T], named type -> &Name, option<T> -> Option<&T'>,
    # result<T, E> -> Result<&T', &E'>. The glue owns the converted value and borrows it.

    def borrowed_inner(self, interface, wit_type, reference, where):
        """Expression of the borrowed form of `wit_type`, given `reference: &Owned`."""
        kind = wit_type[0]
        if kind == "unit":
            return "()"
        if kind == "primitive" and wit_type[1] != "string":
            return f"*{reference}"
        _, core = self.dealias(interface, wit_type)
        if core == ("primitive", "string"):
            return f"{reference}.as_str()"
        if core[0] == "list":
            return f"{reference}.as_slice()"
        if kind == "named":
            return reference
        self.fail(f"{where}: parameter shape {wit_type} inside a result is not supported")
        return None

    def argument(self, interface, wit_type, variable, where):
        if self.has_handle(interface, wit_type):
            self.fail(f"{where}: a handle parameter is not supported")
        if not self.has_list(interface, wit_type):
            return variable
        kind = wit_type[0]
        if kind in ("primitive", "list", "named"):
            return f"&{variable}"
        if kind == "option":
            inner = wit_type[1]
            if self.is_string_or_list(interface, inner):
                return f"{variable}.as_deref()"
            if inner[0] == "named":
                return f"{variable}.as_ref()"
            self.fail(f"{where}: parameter shape {wit_type} is not supported")
        if kind == "result":
            ok = self.borrowed_inner(interface, wit_type[1], "value", where)
            error = self.borrowed_inner(interface, wit_type[2], "error", where)
            return (
                f"match &{variable} {{ ::core::result::Result::Ok(value) => ::core::result::Result::Ok({ok}), "
                f"::core::result::Result::Err(error) => ::core::result::Result::Err({error}) }}"
            )
        self.fail(f"{where}: parameter shape {wit_type} is not supported")
        return None

    # -- conversions

    def conversions(self, interface, definition, context):
        kind = definition["kind"]
        traits = context.traits
        if kind == "resource":
            if interface.directory == "exports":
                return []
            mirror = self.mirror_name(interface, definition, context)
            bindgen = self.bindgen_name(interface, definition, context)
            return [
                f"impl {traits}IntoMirror<{mirror}> for {bindgen} {{",
                f"    fn into_mirror(self) -> {mirror} {{",
                "        // The plugin now owns the handle; it is released through the interface.",
                f"        {mirror}(self.take_handle())",
                "    }",
                "}",
                f"impl {traits}IntoWit<{bindgen}> for {mirror} {{",
                f"    fn into_wit(self) -> {bindgen} {{",
                "        // SAFETY: the handle was produced by `take_handle` of the same resource type.",
                f"        unsafe {{ {bindgen}::from_handle(self.0) }}",
                "    }",
                "}",
            ]
        if kind == "type":
            if not self.renderer.is_id_newtype(interface, definition):
                return []
            mirror = self.mirror_name(interface, definition, context)
            target = self.type_text(interface, definition["target"], context, False)
            return [
                f"impl {traits}IntoWit<{target}> for {mirror} {{",
                f"    fn into_wit(self) -> {target} {{",
                "        self.0",
                "    }",
                "}",
                f"impl {traits}IntoMirror<{mirror}> for {target} {{",
                f"    fn into_mirror(self) -> {mirror} {{",
                f"        {mirror}(self)",
                "    }",
                "}",
            ]
        mirror = self.mirror_name(interface, definition, context)
        bindgen = self.bindgen_name(interface, definition, context)
        if kind != "flags" and self.has_handle(interface, ("named", definition["name"])):
            return [f"// {definition['name']}: holds a resource handle, no conversion yet."]
        to_wit = [f"impl {traits}IntoWit<{bindgen}> for {mirror} {{", f"    fn into_wit(self) -> {bindgen} {{"]
        to_mirror = [f"impl {traits}IntoMirror<{mirror}> for {bindgen} {{", f"    fn into_mirror(self) -> {mirror} {{"]
        if kind == "record":
            to_wit.append(f"        {bindgen} {{")
            to_mirror.append(f"        {mirror} {{")
            for field in definition["fields"]:
                mirror_field = self.api.rust_value_name(field["name"])
                bindgen_field = self.bindgen_ident(field["name"])
                to_wit.append(f"            {bindgen_field}: {traits}IntoWit::into_wit(self.{mirror_field}),")
                to_mirror.append(f"            {mirror_field}: {traits}IntoMirror::into_mirror(self.{bindgen_field}),")
            to_wit.append("        }")
            to_mirror.append("        }")
        elif kind in ("enum", "variant"):
            to_wit.append("        match self {")
            to_mirror.append("        match self {")
            for case in definition["cases"]:
                mirror_case = f"{mirror}::{self.api.rust_case_name(case['name'])}"
                bindgen_case = f"{bindgen}::{self.bindgen_camel(case['name'])}"
                if kind == "variant" and case["payload"] is not None:
                    to_wit.append(f"            {mirror_case}(value) => {bindgen_case}({traits}IntoWit::into_wit(value)),")
                    to_mirror.append(
                        f"            {bindgen_case}(value) => {mirror_case}({traits}IntoMirror::into_mirror(value)),"
                    )
                else:
                    to_wit.append(f"            {mirror_case} => {bindgen_case},")
                    to_mirror.append(f"            {bindgen_case} => {mirror_case},")
            to_wit.append("        }")
            to_mirror.append("        }")
        elif kind == "flags":
            to_wit.append(f"        {bindgen}::from_bits_retain(self.0 as _)")
            to_mirror.append(f"        {mirror}(self.bits() as u32)")
        else:
            self.fail(f"unknown definition kind {kind}")
        to_wit.extend(["    }", "}"])
        to_mirror.extend(["    }", "}"])
        return to_wit + to_mirror

    # -- functions of an interface, as the mirror trait lists them

    def methods(self, interface):
        result = [(function, None) for function in interface.functions]
        for definition in interface.types:
            if definition["kind"] == "resource":
                for method in definition["methods"]:
                    result.append((method, definition))
        return result

    def mirror_method(self, function, resource):
        if resource is None:
            return self.api.rust_value_name(function["name"])
        if function["kind"] == "constructor":
            return self.api.rust_value_name(f"{resource['name']}-new")
        return self.api.rust_value_name(f"{resource['name']}-{function['name']}")

    def mirror_signature(self, interface, function, resource, context):
        """(parameter list text without self, result text) with positional names a0, a1, ..."""
        parameters = []
        if resource is not None and function["kind"] == "method":
            parameters.append(f"handle: {self.mirror_name(interface, resource, context)}")
        for index, (_, parameter_type) in enumerate(function["parameters"]):
            parameters.append(f"a{index}: {self.type_text(interface, parameter_type, context, True)}")
        if resource is not None and function["kind"] == "constructor":
            result = self.mirror_name(interface, resource, context)
        elif function["result"] is None:
            result = "()"
        else:
            result = self.type_text(interface, function["result"], context, True)
        return parameters, result

    def call_names(self, function, resource):
        names = []
        if resource is not None and function["kind"] == "method":
            names.append("handle")
        names.extend(f"a{index}" for index in range(len(function["parameters"])))
        return names

    # -- imports

    def import_function(self, interface, function, resource):
        context = IN_SDK
        where = f"{interface.wit_path}.{function['name']}"
        parameters, result = self.mirror_signature(interface, function, resource, context)
        name = self.mirror_method(function, resource)
        lines = [f"        pub fn {name}({', '.join(parameters)}) -> {result} {{"]
        arguments = []
        for index, (_, parameter_type) in enumerate(function["parameters"]):
            bindgen_type = self.type_text(interface, parameter_type, context, False)
            lines.append(f"            let w{index}: {bindgen_type} = IntoWit::into_wit(a{index});")
            arguments.append(self.argument(interface, parameter_type, f"w{index}", where))
        joined = ", ".join(arguments)
        module = self.bindgen_module(interface, context)
        if resource is None:
            lines.append(f"            let out = {module}::{self.bindgen_ident(function['name'])}({joined});")
        else:
            resource_type = self.bindgen_name(interface, resource, context)
            if function["kind"] == "constructor":
                lines.append(f"            let out = {resource_type}::new({joined});")
            elif function["kind"] == "static":
                lines.append(f"            let out = {resource_type}::{self.bindgen_ident(function['name'])}({joined});")
            else:
                lines.append("            // SAFETY: the handle came from this resource type; it stays owned by the plugin,")
                lines.append("            // so the temporary wrapper is forgotten instead of dropped.")
                lines.append(f"            let resource = unsafe {{ {resource_type}::from_handle(handle.0) }};")
                lines.append(f"            let out = resource.{self.bindgen_ident(function['name'])}({joined});")
                lines.append("            ::core::mem::forget(resource);")
        lines.append("            IntoMirror::into_mirror(out)")
        lines.append("        }")
        return lines

    def host_call_interfaces(self):
        """The interfaces behind `HostCalls` in plugin.rs, in its order."""
        text = (self.root / SDK_CORE / "src" / "plugin.rs").read_text(encoding="utf-8")
        match = re.search(r"pub trait HostCalls:(.*?)\{", text, re.S)
        if match is None:
            self.fail("plugin.rs has no `pub trait HostCalls:`")
        traits = re.findall(r"Host[A-Za-z0-9]+", match.group(1))
        by_trait = {}
        for key in self.model.order:
            interface = self.model.interfaces[key]
            if interface.directory in IMPORT_DIRECTORIES and interface.directory != "types":
                by_trait[self.renderer.trait_name(interface)] = interface
        interfaces = []
        for trait in traits:
            if trait not in by_trait:
                self.fail(f"HostCalls names {trait}, which no interface defines")
            interfaces.append(by_trait[trait])
        return interfaces

    def sides_importing(self, interface):
        package = f"{self.api.PACKAGE_OF_DIRECTORY[interface.directory]}@{self.api.VERSION}"
        sides = []
        for side, world_package in WORLD_OF_SIDE:
            for owner, world in self.model.worlds:
                if owner != world_package or world["name"] != "base":
                    continue
                for entry in world["entries"]:
                    if entry["kind"] == "import" and entry["package"] == package and entry["interface"] == interface.name:
                        sides.append(side)
        return sides

    def host_macro_name(self, interface):
        return f"__ilium_wasm_host_{self.api.snake(interface.name)}"

    def host_macro(self, interface):
        context = IN_MACRO
        macro = self.host_macro_name(interface)
        trait = self.mirror_trait(interface, context)
        on_lines = []
        off_lines = []
        for function, resource in self.methods(interface):
            parameters, result = self.mirror_signature(interface, function, resource, context)
            name = self.mirror_method(function, resource)
            names = ", ".join(self.call_names(function, resource))
            signature = f"            fn {name}(&mut self, {', '.join(parameters)}) -> {result} {{"
            if not parameters:
                signature = f"            fn {name}(&mut self) -> {result} {{"
            on_lines.append(signature)
            on_lines.append(f"                $crate::wasm::imports::{self.api.snake(interface.name)}::{name}({names})")
            on_lines.append("            }")
            off_lines.append(signature)
            separator = ", " if names else ""
            off_lines.append(
                f"                <$crate::HostHandle as {trait}>::{name}(&mut $crate::wasm::detached(){separator}{names})"
            )
            off_lines.append("            }")
        sides = self.sides_importing(interface)
        lines = [
            f"/// `{interface.wit_path}` for the host type of a component: `on` calls the imports, `off`",
            "/// answers like a thread without a host and references no import.",
            "#[doc(hidden)]",
            "#[macro_export]",
            f"macro_rules! {macro} {{",
            "    (on $host:ident) => {",
            f"        impl {trait} for $host {{",
        ]
        lines.extend(on_lines)
        lines.extend(["        }", "    };", "    (off $host:ident) => {", f"        impl {trait} for $host {{"])
        lines.extend(off_lines)
        lines.extend(["        }", "    };"])
        for side, _ in WORLD_OF_SIDE:
            state = "on" if side in sides else "off"
            lines.append(f"    ({side} $host:ident) => {{")
            lines.append(f"        $crate::{macro}!({state} $host);")
            lines.append("    };")
        lines.append("}")
        return lines

    # -- exports

    def export_interfaces(self):
        return [
            self.model.interfaces[key] for key in self.model.order
            if key[0] == "exports" and key[1] not in EXPORTS_WITHOUT_GLUE
        ]

    def export_macro_name(self, interface):
        return f"__ilium_wasm_export_{self.api.snake(interface.name)}"

    def export_guest(self, interface):
        context = IN_MACRO
        guest = f"{self.bindgen_module(interface, context)}::Guest"
        trait = self.mirror_trait(interface, context)
        lines = [f"        impl {guest} for __IliumGlue {{"]
        for function, resource in self.methods(interface):
            if resource is not None:
                self.fail(f"{interface.wit_path}: exported resources have no glue")
            where = f"{interface.wit_path}.{function['name']}"
            parameters = []
            for index, (_, parameter_type) in enumerate(function["parameters"]):
                if self.has_handle(interface, parameter_type):
                    self.fail(f"{where}: a handle parameter is not supported")
                parameters.append(f"a{index}: {self.type_text(interface, parameter_type, context, False)}")
            result = "()" if function["result"] is None else self.type_text(interface, function["result"], context, False)
            lines.append(f"            fn {self.bindgen_ident(function['name'])}({', '.join(parameters)}) -> {result} {{")
            if interface.name == "lifecycle" and function["name"] == "activate":
                lines.append("                __ilium_install_host();")
            names = []
            for index, (_, parameter_type) in enumerate(function["parameters"]):
                mirror_type = self.type_text(interface, parameter_type, context, True)
                lines.append(f"                let m{index}: {mirror_type} = $crate::wasm::IntoMirror::into_mirror(a{index});")
                names.append(f"m{index}")
            separator = ", " if names else ""
            method = self.mirror_method(function, None)
            if interface.name == "handler":
                # Every component exports the handler; a plugin without a `Handler`
                # implementation answers with no submission (see `HandlerProbe`).
                lines.append("                let out = __ilium_with(|plugin| {")
                lines.append("                    use $crate::{DispatchToHandler as _, DispatchToNobody as _};")
                lines.append(f"                    (&mut $crate::HandlerProbe(plugin)).dispatch_events({', '.join(names)})")
                lines.append("                });")
            else:
                lines.append(
                    f"                let out = __ilium_with(|plugin| <__IliumPlugin as {trait}>::{method}(plugin{separator}{', '.join(names)}));"
                )
            lines.append("                $crate::wasm::IntoWit::into_wit(out)")
            lines.append("            }")
        lines.append("        }")
        return lines

    def export_macro(self, interface):
        macro = self.export_macro_name(interface)
        conversions = []
        for definition in interface.types:
            for line in self.conversions(interface, definition, IN_MACRO):
                conversions.append("        " + line)
        lines = [
            f"/// Guest side of `{interface.wit_path}`: forwards to the plugin's `{self.renderer.trait_name(interface)}`.",
            "/// Expanded by `#[ilium_plugin]` next to `bindings`, `__IliumGlue`, `__IliumPlugin`,",
            "/// `__ilium_with` and `__ilium_install_host`.",
            "#[doc(hidden)]",
            "#[macro_export]",
            f"macro_rules! {macro} {{",
            "    () => {",
        ]
        lines.extend(conversions)
        lines.extend(self.export_guest(interface))
        lines.append("    };")
        lines.append("}")
        return lines

    def import_closure(self, interface):
        """Every import interface the export interface needs, directly or through `use`."""
        seen = []
        queue = [interface]
        while queue:
            current = queue.pop(0)
            for use in current.uses:
                directory = current.directory if use["package"] is None else self.model.directory_of_package(use["package"])
                source = self.model.interfaces[(directory, use["interface"])]
                if source.directory == "exports":
                    self.fail(f"{interface.wit_path} uses the export interface {source.wit_path}")
                if source not in seen:
                    seen.append(source)
                    queue.append(source)
        return sorted(seen, key=self.wit_id)

    def sides_exporting(self, interface):
        package = f"{self.api.PACKAGE_OF_DIRECTORY['exports']}@{self.api.VERSION}"
        sides = []
        for side, world_package in WORLD_OF_SIDE:
            for owner, world in self.model.worlds:
                if owner != world_package:
                    continue
                for entry in world["entries"]:
                    if entry["kind"] == "export" and entry["package"] == package and entry["interface"] == interface.name:
                        if side not in sides:
                            sides.append(side)
        return sides

    # -- files

    def world_text(self):
        lines = [f"package ilium:sdk-glue@{self.api.VERSION};", "", "world imports {"]
        for key in self.model.order:
            interface = self.model.interfaces[key]
            if interface.directory in IMPORT_DIRECTORIES:
                lines.append(f"    import {self.wit_id(interface)};")
        lines.append("}")
        return lines

    def glue_file(self, header_lines):
        lines = list(header_lines)
        lines.extend([
            "//! wasm32 guest glue: wit-bindgen bindings of every import interface, the conversions to",
            "//! and from the mirror types, the import wrappers and the macros `#[ilium_plugin]` expands.",
            "//! Generated by tools/gen_plugin_api.py (tools/gen_plugin_wasm_glue.py). Never edit by hand.",
            "#![allow(",
            "    clippy::unnecessary_cast,",
            "    clippy::too_many_arguments,",
            "    clippy::needless_borrow,",
            "    clippy::let_unit_value,",
            "    clippy::unit_arg,",
            "    clippy::useless_conversion,",
            "    clippy::type_complexity,",
            "    clippy::large_enum_variant,",
            "    clippy::missing_safety_doc,",
            "    clippy::let_and_return",
            ")]",
            "",
            "use super::{IntoMirror, IntoWit};",
            "use crate::generated as m;",
            "",
            "/// The wit-bindgen bindings of every import interface. Imports no plugin calls are dropped",
            "/// when the component is linked.",
            "#[allow(clippy::all, missing_docs, unused, dead_code)]",
            "pub mod bindings {",
            "    wit_bindgen::generate!({",
            '        inline: r#"',
        ])
        lines.extend(self.world_text())
        lines.extend([
            '"#,',
            "        path: [" + ", ".join(f'"../wit/{directory}"' for directory in IMPORT_DIRECTORIES) + "],",
            "        generate_all,",
            "    });",
            "}",
            "",
            "use bindings as b;",
        ])
        for key in self.model.order:
            interface = self.model.interfaces[key]
            if interface.directory not in IMPORT_DIRECTORIES:
                continue
            block = []
            for definition in interface.types:
                block.extend(self.conversions(interface, definition, IN_SDK))
            if block:
                lines.append("")
                lines.append(f"// {interface.wit_path}")
                lines.extend(block)
        host_interfaces = self.host_call_interfaces()
        lines.extend([
            "",
            "/// One function per host function a plugin can call through `host()`.",
            "pub mod imports {",
            "    use super::{b, m, IntoMirror, IntoWit};",
        ])
        for interface in host_interfaces:
            lines.append("")
            lines.append(f"    /// `{interface.wit_path}`.")
            lines.append(f"    pub mod {self.api.snake(interface.name)} {{")
            lines.append("        use super::{b, m, IntoMirror, IntoWit};")
            for function, resource in self.methods(interface):
                lines.append("")
                lines.extend(self.import_function(interface, function, resource))
            lines.append("    }")
        lines.append("}")
        for interface in host_interfaces:
            lines.append("")
            lines.extend(self.host_macro(interface))
        for interface in self.export_interfaces():
            lines.append("")
            lines.extend(self.export_macro(interface))
        return "\n".join(lines) + "\n"

    def table_file(self, header_lines):
        lines = list(header_lines)
        lines.extend([
            "//! Tables for `#[ilium_plugin]`: what the wasm32 glue of a component is made of.",
            "//! Generated by tools/gen_plugin_api.py (tools/gen_plugin_wasm_glue.py). Never edit by hand.",
            "",
            "/// The WIT package directories under `wit/`, dependencies first.",
            "pub(crate) const WIT_DIRECTORIES: &[&str] = &["
            + ", ".join(f'"{directory}"' for directory in IMPORT_DIRECTORIES + ("exports",)) + "];",
            "",
            "/// One export interface with glue.",
            "pub(crate) struct ExportGlue {",
            "    /// Name in the `exports = [...]` argument and in the manifest.",
            "    pub(crate) name: &'static str,",
            "    /// The versioned WIT id.",
            "    pub(crate) wit: &'static str,",
            "    /// The sdk-core macro that implements its `Guest` trait.",
            "    pub(crate) glue_macro: &'static str,",
            "    /// Sides whose worlds can export it.",
            "    pub(crate) sides: &'static [&'static str],",
            "    /// Import interfaces it needs: (versioned WIT id, module path below the bindings root).",
            "    pub(crate) with: &'static [(&'static str, &'static str)],",
            "}",
            "",
            "pub(crate) const EXPORTS: &[ExportGlue] = &[",
        ])
        for interface in self.export_interfaces():
            lines.append("    ExportGlue {")
            lines.append(f'        name: "{interface.name}",')
            lines.append(f'        wit: "{self.wit_id(interface)}",')
            lines.append(f'        glue_macro: "{self.export_macro_name(interface)}",')
            lines.append("        sides: &[" + ", ".join(f'"{side}"' for side in self.sides_exporting(interface)) + "],")
            lines.append("        with: &[")
            for source in self.import_closure(interface):
                lines.append(f'            ("{self.wit_id(source)}", "{self.bindgen_module_suffix(source)}"),')
            lines.append("        ],")
            lines.append("    },")
        lines.extend([
            "];",
            "",
            "/// Export interfaces that exist in the contract but have no wasm32 glue yet.",
            "pub(crate) const EXPORTS_WITHOUT_GLUE: &[&str] = &["
            + ", ".join(f'"{name}"' for name in EXPORTS_WITHOUT_GLUE) + "];",
            "",
            "/// One host interface behind `host()`.",
            "pub(crate) struct HostGlue {",
            "    /// Interface name; a grant `<name>` or `<name>.<rest>` permits the import.",
            "    pub(crate) grant: &'static str,",
            "    /// The sdk-core macro that implements its `Host*` trait.",
            "    pub(crate) glue_macro: &'static str,",
            "    /// True when the host links it without any grant.",
            "    pub(crate) always: bool,",
            "}",
            "",
            "pub(crate) const HOST_CALLS: &[HostGlue] = &[",
        ])
        for interface in self.host_call_interfaces():
            always = interface.directory != "host" or interface.name in ALWAYS_ALLOWED
            lines.append("    HostGlue {")
            lines.append(f'        grant: "{interface.name}",')
            lines.append(f'        glue_macro: "{self.host_macro_name(interface)}",')
            lines.append(f"        always: {'true' if always else 'false'},")
            lines.append("    },")
        lines.append("];")
        return "\n".join(lines) + "\n"


def outputs(api, model, renderer, root, tree_sha256):
    """{relative path: unformatted text} of the glue files."""
    glue = Glue(api, model, renderer, root)
    return {
        GLUE_PATH: glue.glue_file(api.header(tree_sha256, GLUE_PATH)),
        TABLE_PATH: glue.table_file(api.header(tree_sha256, TABLE_PATH)),
    }
