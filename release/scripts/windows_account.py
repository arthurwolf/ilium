"""Read native installer/account state; restore only an acknowledged PATH state."""  # Limit this adapter's authority.
from __future__ import annotations  # Keep portable deferred annotations.
import ctypes  # Bind installed Windows APIs.
from ctypes import wintypes  # Declare native argument widths.
import ntpath  # Normalize Windows paths portably.
import os  # Reject non-Windows native calls.
from pathlib import Path  # Expose verified filesystem locations.
import re  # Validate narrow native identifiers.
msi_upgrade_code = '{6F1B7D0E-2C54-4A9B-9E3D-51C8A4B7F0D2}'  # Preserve the authored UpgradeCode.
inno_key = r'Software\Microsoft\Windows\CurrentVersion\Uninstall\{C3A92E58-7B16-4D0F-8A41-0E5D9F2B6C73}_is1'  # Preserve the authored AppId.
path_key = r'Environment'  # Identify the account environment.
machine_path_key = r'SYSTEM\CurrentControlSet\Control\Session Manager\Environment'  # Observe the machine environment.
inno_values = ('DisplayName', 'DisplayVersion', 'InstallLocation', 'UninstallString', 'QuietUninstallString', 'Publisher')  # Bound registration readbacks.
def require(condition: bool, message: str) -> None:  # Raise portable contract failures.
    if condition:  # Return before the failure branch.
        return  # Preserve the admitted state.
    raise ValueError(message)  # Fail without native mutation.
def normalized_path(value: str) -> str:  # Normalize comparison without rewriting data.
    return ntpath.normcase(ntpath.normpath(value.strip().strip('"').replace('/', '\\')))  # Retain raw snapshots separately.
class windows_account:  # Encapsulate narrowly scoped account access.
    def __init__(self) -> None:  # Initialize only on supported Windows.
        require(os.name == 'nt' and ctypes.sizeof(ctypes.c_void_p) == 8, 'native smoke requires 64-bit Windows Python')  # Refuse unsupported hosts.
        import winreg  # Delay the Windows-only import.
        self.winreg = winreg  # Retain the registry adapter.
        kernel = ctypes.WinDLL('kernel32.dll', use_last_error=True)  # Load the Windows known DLL.
        kernel.GetSystemDirectoryW.argtypes = (wintypes.LPWSTR, wintypes.UINT)  # Declare the directory query.
        kernel.GetSystemDirectoryW.restype = wintypes.UINT  # Preserve the native result width.
        system_buffer = ctypes.create_unicode_buffer(32768)  # Bound the authoritative path.
        length = kernel.GetSystemDirectoryW(system_buffer, len(system_buffer))  # Read the native system directory.
        require(0 < length < len(system_buffer), 'GetSystemDirectoryW failed or exceeded its buffer')  # Reject missing authority.
        system_directory = Path(system_buffer.value)  # Retain the authoritative location.
        self.system_directory = system_directory  # Expose the native tool directory to the gate.
        shell = ctypes.WinDLL(str(system_directory / 'shell32.dll'))  # Load known-folder support absolutely.
        shell.SHGetFolderPathW.argtypes = (wintypes.HWND, ctypes.c_int, wintypes.HANDLE, wintypes.DWORD, wintypes.LPWSTR)  # Declare the account folder query.
        shell.SHGetFolderPathW.restype = ctypes.c_long  # Preserve the HRESULT result.
        local_buffer = ctypes.create_unicode_buffer(260)  # Match the documented MAX_PATH buffer.
        require(shell.SHGetFolderPathW(None, 0x001c, None, 0, local_buffer) == 0, 'cannot read current account LocalAppData')  # Avoid environment-root guesses.
        security = ctypes.WinDLL(str(system_directory / 'advapi32.dll'), use_last_error=True)  # Load account identity support.
        security.GetUserNameW.argtypes = (wintypes.LPWSTR, ctypes.POINTER(wintypes.DWORD))  # Declare the account-name query.
        security.GetUserNameW.restype = wintypes.BOOL  # Preserve native success semantics.
        user_buffer = ctypes.create_unicode_buffer(32768)  # Bound the current identity.
        user_length = wintypes.DWORD(len(user_buffer))  # Supply the destination capacity.
        require(bool(security.GetUserNameW(user_buffer, ctypes.byref(user_length))), 'cannot read current Windows user')  # Fail before claiming account custody.
        self.identity = {'user': user_buffer.value, 'local_app_data': local_buffer.value, 'system_directory': str(system_directory)}  # Retain authoritative readbacks.
        self.directory = Path(local_buffer.value) / 'Programs' / 'ilium'  # Preserve default installer placement.
        self.kernel = kernel  # Keep the API module alive.
        kernel.ExpandEnvironmentStringsW.argtypes = (wintypes.LPCWSTR, wintypes.LPWSTR, wintypes.DWORD)  # Declare token expansion.
        kernel.ExpandEnvironmentStringsW.restype = wintypes.DWORD  # Preserve required-buffer reporting.
        self.msi = ctypes.WinDLL(str(system_directory / 'msi.dll'))  # Load native registration queries absolutely.
        self.msi.MsiEnumRelatedProductsW.argtypes = (wintypes.LPCWSTR, wintypes.DWORD, wintypes.DWORD, wintypes.LPWSTR)  # Declare related-product enumeration.
        self.msi.MsiEnumRelatedProductsW.restype = wintypes.UINT  # Preserve Windows Installer errors.
        self.msi.MsiQueryProductStateW.argtypes = (wintypes.LPCWSTR,)  # Declare retained-product readback.
        self.msi.MsiQueryProductStateW.restype = ctypes.c_int  # Preserve negative installation states.
        self.msi.MsiGetProductInfoW.argtypes = (wintypes.LPCWSTR, wintypes.LPCWSTR, wintypes.LPWSTR, ctypes.POINTER(wintypes.DWORD))  # Declare version-registration readback.
        self.msi.MsiGetProductInfoW.restype = wintypes.UINT  # Preserve query failure codes.
    def _value(self, key, name: str) -> dict:  # Read exact registry value state.
        try:  # Distinguish an absent value.
            value, value_type = self.winreg.QueryValueEx(key, name)  # Retain raw value and type.
        except FileNotFoundError:  # Absence is a valid readback.
            return {'exists': False, 'type': None, 'value': None}  # Preserve absent versus empty.
        require(value_type in (self.winreg.REG_SZ, self.winreg.REG_EXPAND_SZ) and isinstance(value, str), 'unexpected registry string type: ' + name)  # Reject unsupported registry data.
        return {'exists': True, 'type': value_type, 'value': value}  # Avoid expansion or normalization.
    def _key(self, hive, path: str, view: int, names: tuple[str, ...]) -> dict:  # Read only named registration fields.
        try:  # Distinguish an absent key.
            key = self.winreg.OpenKey(hive, path, 0, self.winreg.KEY_READ | view)  # Request explicit registry view.
        except FileNotFoundError:  # Preserve missing registration as evidence.
            return {'exists': False, 'values': {}}  # Do not swallow access failures.
        with key:  # Close every registry handle.
            return {'exists': True, 'values': {name: self._value(key, name) for name in names}}  # Capture selected raw fields.
    def _path(self, hive, path: str) -> dict:  # Read one environment PATH value.
        state = self._key(hive, path, self.winreg.KEY_WOW64_64KEY, ('Path',))  # Avoid inherited process PATH.
        if not state['exists']:  # Preserve absent environment-key state.
            return {'exists': False, 'type': None, 'value': None}  # Report no PATH value.
        return state['values']['Path']  # Return the unexpanded typed value.
    def read_path(self) -> dict:  # Observe current-user PATH.
        return self._path(self.winreg.HKEY_CURRENT_USER, path_key)  # Read the actual account hive.
    def machine_path(self) -> dict:  # Observe machine PATH without writing.
        return self._path(self.winreg.HKEY_LOCAL_MACHINE, machine_path_key)  # Retain the independent baseline.
    def path_has(self, value: str, directory: str) -> bool:  # Detect authored equivalent directory tokens.
        wanted = normalized_path(directory)  # Normalize only the comparison target.
        for token in value.split(';'):  # Preserve tokens in the original snapshot.
            if not token.strip():  # Ignore empty search tokens for membership.
                continue  # Keep authored empties untouched.
            buffer = ctypes.create_unicode_buffer(32768)  # Bound each native expansion.
            length = self.kernel.ExpandEnvironmentStringsW(token.strip().strip('"'), buffer, len(buffer))  # Expand the current account token.
            require(0 < length <= len(buffer), 'PATH token expansion failed or exceeded bound')  # Refuse ambiguous membership.
            if normalized_path(buffer.value) == wanted:  # Compare quoted and slash variants.
                return True  # A pre-existing equivalent must be preserved.
        return False  # No matching token was observed.
    def product_state(self, product_code: str) -> int:  # Query an already retained product identity.
        require(re.fullmatch(r'\{[0-9A-Fa-f]{8}(?:-[0-9A-Fa-f]{4}){3}-[0-9A-Fa-f]{12}\}', product_code) is not None, 'invalid MSI product identity')  # Reject arbitrary product selectors.
        return int(self.msi.MsiQueryProductStateW(product_code))  # Preserve UNKNOWN versus other-user ABSENT.
    def product_version(self, product_code: str) -> str:  # Observe the installed MSI version.
        buffer = ctypes.create_unicode_buffer(4096)  # Bound the version property buffer.
        length = wintypes.DWORD(len(buffer))  # Supply capacity including terminator.
        result = int(self.msi.MsiGetProductInfoW(product_code, 'VersionString', buffer, ctypes.byref(length)))  # Query registered ProductVersion.
        require(result == 0 and length.value < len(buffer), 'MSI VersionString query failed: ' + str(result))  # Reject absent or partial readbacks.
        return buffer.value  # Preserve exact registered version text.
    def registrations(self) -> dict:  # Observe only this package's registration.
        products = []  # Retain the exact related-product set.
        for index in range(64):  # Bound enumeration even on damaged hosts.
            product = ctypes.create_unicode_buffer(39)  # Allocate the documented GUID buffer.
            result = int(self.msi.MsiEnumRelatedProductsW(msi_upgrade_code, 0, index, product))  # Enumerate on this calling thread.
            if result == 259:  # ERROR_NO_MORE_ITEMS completes the inventory.
                break  # Preserve all preceding products.
            require(result == 0, 'MSI related-product enumeration failed: ' + str(result))  # Fail on partial inventories.
            state = self.product_state(product.value)  # Read exact native product state.
            products.append({'product_code': product.value, 'state': state, 'version': self.product_version(product.value) if state == 5 else None})  # Retain the installed version registration.
        else:  # Exhausting the bound is not successful enumeration.
            raise ValueError('MSI related-product inventory exceeded bound')  # Preserve existing registrations.
        custom = self._key(self.winreg.HKEY_CURRENT_USER, r'Software\Ilium', self.winreg.KEY_WOW64_64KEY, ('InstallDirectory',))  # Observe the MSI-owned key.
        inno = {}  # Retain every relevant registry view.
        for hive_name, hive in (('HKCU', self.winreg.HKEY_CURRENT_USER), ('HKLM', self.winreg.HKEY_LOCAL_MACHINE)):  # Detect user and machine collisions.
            for bits, view in ((32, self.winreg.KEY_WOW64_32KEY), (64, self.winreg.KEY_WOW64_64KEY)):  # Read each explicit view.
                inno[hive_name + str(bits)] = self._key(hive, inno_key, view, inno_values)  # Preserve absent and populated registrations.
        return {'msi_products': products, 'msi_key': custom, 'inno': inno}  # Return a JSON-compatible bounded inventory.
    def snapshot(self) -> dict:  # Capture authoritative account acceptance state.
        return {'user_path': self.read_path(), 'machine_path': self.machine_path(), 'registrations': self.registrations()}  # Keep native sources separate.
    def inno_path_receipt(self) -> dict:
        """Read the EXE's durable ownership proof from its explicit per-user view."""
        with self.winreg.OpenKey(self.winreg.HKEY_CURRENT_USER, inno_key, 0, self.winreg.KEY_READ | self.winreg.KEY_WOW64_64KEY) as key:
            result = {}
            for name in ('IliumPathReceipt', 'IliumPathExisted', 'IliumPathOwned'):
                value, kind = self.winreg.QueryValueEx(key, name)
                require(kind == self.winreg.REG_DWORD and type(value) is int, 'invalid EXE receipt flag: ' + name)
                result[name] = value
            for name in ('IliumPathBefore', 'IliumPathAfter', 'IliumPathEntry'):
                value = self._value(key, name)
                require(value['exists'] and value['type'] == self.winreg.REG_SZ, 'invalid EXE receipt text: ' + name)
                result[name] = value['value']
        return result
    def require_clean(self, snapshot: dict) -> None:  # Refuse existing installations before mutation.
        registrations = snapshot['registrations']  # Inspect the captured package registrations.
        require(not registrations['msi_products'] and not registrations['msi_key']['exists'], 'smoke account has existing MSI registration or Ilium key')  # Preserve existing product ownership.
        require(not any(item['exists'] for item in registrations['inno'].values()), 'smoke account has existing Inno registration')  # Preserve either-view installations.
    def validate_installed(self, kind: str, version: str, directory: Path, snapshot: dict) -> dict:  # Validate installed registration and return stable custody.
        registrations = snapshot['registrations']  # Consume the authoritative snapshot.
        products, custom, inno = registrations['msi_products'], registrations['msi_key'], registrations['inno']  # Bind the relevant registration groups.
        if kind.upper() == 'MSI':  # Validate the single authored MSI product.
            require(len(products) == 1 and products[0]['state'] == 5, 'MSI must have exactly one currently installed product')  # Reject advertised or other-user products.
            require(products[0]['version'] == version, 'MSI VersionString differs from the audited release')  # Reject incorrect registered product versions.
            require(custom['exists'], 'MSI InstallDirectory registration is absent')  # Require the authored key path.
            value = custom['values']['InstallDirectory']  # Retain its typed raw value.
            require(value['exists'] and value['type'] == self.winreg.REG_SZ and normalized_path(value['value']) == normalized_path(str(directory)), 'MSI InstallDirectory readback differs')  # Bind registration to installed files.
            require(not any(item['exists'] for item in inno.values()), 'MSI install introduced Inno registration')  # Reject cross-format residue.
            return {'kind': 'MSI', 'product_code': products[0]['product_code']}  # Retain exact cleanup/product identity.
        require(kind.upper() == 'EXE', 'unknown installer format')  # Refuse unsupported format contracts.
        require(not products and not custom['exists'], 'EXE install has unexpected MSI registration')  # Reject previous-format residue.
        require(not inno['HKLM32']['exists'] and not inno['HKLM64']['exists'], 'EXE wrote machine installation registration')  # Preserve per-user scope.
        current = inno['HKCU64']  # Select the authored non-administrative registration.
        require(current['exists'], 'EXE uninstall registration is absent')  # Require actual registered installation.
        require(not inno['HKCU32']['exists'] or inno['HKCU32'] == current, 'EXE registry views disagree')  # Permit the shared HKCU alias only.
        values = current['values']  # Inspect the recorded semantic fields.
        for name, expected in (('DisplayName', 'Ilium'), ('DisplayVersion', version), ('Publisher', 'Arthur Wolf')):  # Bind product identity and exact version.
            require(values[name]['exists'] and values[name]['value'] == expected, 'EXE ' + name + ' readback differs')  # Reject substring identity matches.
        location = values['InstallLocation']  # Preserve the literal recorded directory.
        require(location['exists'] and normalized_path(location['value']) == normalized_path(str(directory)), 'EXE InstallLocation readback differs')  # Bind registration to the default install.
        uninstall = values['UninstallString']  # Treat registered commands only as data.
        match = re.fullmatch(r'"([^"]+)"', uninstall['value'] or '')  # Accept the supplied default quoted executable only.
        require(uninstall['exists'] and match is not None, 'EXE uninstall command is not one quoted executable')  # Refuse injected arguments or ambiguity.
        executable = match.group(1)  # Retain the validated path without executing it.
        require(normalized_path(ntpath.dirname(executable)) == normalized_path(str(directory)) and ntpath.basename(executable).casefold() == 'unins000.exe', 'EXE uninstall path differs from the clean-install contract')  # Bind the exact uninstaller inventory.
        return {'kind': 'EXE', 'uninstaller': executable}  # Caller must additionally verify file custody.
    def restore_path(self, original: dict, allowed_current_states: list[dict]) -> None:  # Restore only an acknowledged exact mutation.
        current = self.read_path()  # Re-read before proposing rollback.
        if current == original:  # Avoid needless registry writes.
            return  # The original state is already restored.
        require(current in allowed_current_states, 'PATH changed outside acknowledged installer states; preserved')  # Refuse concurrent or ambiguous edits.
        require(self.read_path() == current, 'PATH changed during cleanup precondition; preserved')  # Recheck immediately before opening for write.
        with self.winreg.OpenKey(self.winreg.HKEY_CURRENT_USER, path_key, 0, self.winreg.KEY_QUERY_VALUE | self.winreg.KEY_SET_VALUE | self.winreg.KEY_WOW64_64KEY) as key:  # Never create or broadly clean keys.
            require(self._value(key, 'Path') == current, 'PATH changed before cleanup write; preserved')  # Recheck through the final handle.
            if original['exists']:  # Restore a known existing typed value.
                require(original['type'] in (self.winreg.REG_SZ, self.winreg.REG_EXPAND_SZ) and isinstance(original['value'], str), 'invalid original PATH snapshot')  # Reject malformed rollback data.
                self.winreg.SetValueEx(key, 'Path', 0, original['type'], original['value'])  # Restore only this acknowledged value.
            else:  # Preserve the original absence distinctly.
                self.winreg.DeleteValue(key, 'Path')  # Delete only the known task-created value.
        require(self.read_path() == original, 'PATH cleanup readback differs; concurrent state may remain')  # Surface restoration/readback failure.
