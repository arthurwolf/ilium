"""Select product integration targets for the explicitly plugin-free Linux preview.

Cargo package exclusions cannot exclude plugin-preview tests embedded in product
crates. Use Cargo's authoritative target inventory, retaining every other test.
The full release never calls this selector and still runs the entire workspace.
"""

# These mixed-target cases assert the future plugin-owned CLI/package contract.
# Product animation probes and all other CLI compatibility cases remain selected.
PREVIEW_ONLY_CASES = (
    'reference_cli_compatibility::',
    'cli_reference::f_111_hidden_commands_work_with_no_plugins',
    'animations::f_102_an_old_format_iliumanim_package_is_refused_with_package_old_format',
)


def preview_integration_arguments(metadata):
    """Return explicit Cargo targets; refuse missing/empty workspace inventories."""
    members = metadata.get('workspace_members', [])
    packages = metadata.get('packages', [])
    if not members or not isinstance(packages, list):
        raise ValueError('preview integration scope requires workspace metadata')
    by_id = {package['id']: package for package in packages}
    if len(set(members)) != len(members) or not set(members).issubset(by_id):
        raise ValueError('preview integration scope has missing or duplicate members')
    targets = set()
    for identity in members:
        package = by_id[identity]
        if package['name'].startswith('ilium-plugin-'):
            continue
        for target in package['targets']:
            if 'test' in target['kind'] and not target['name'].startswith('plugin_'):
                targets.add(target['name'])
    if not targets:
        raise ValueError('preview integration scope cannot omit every product target')
    arguments = [argument for name in sorted(targets) for argument in ('--test', name)]
    # Libtest filters are applied only after Cargo has retained all product targets.
    # They affect the mixed cli_compat target, not whole product crates or modules.
    if 'cli_compat' in targets:
        arguments.append('--')
        for case in PREVIEW_ONLY_CASES:
            arguments.extend(('--skip', case))
    return arguments
