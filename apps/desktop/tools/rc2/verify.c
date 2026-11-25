#include <CoreFoundation/CoreFoundation.h>
#include <Security/Security.h>
#include <node_api.h>
#include <stdbool.h>
#include <stdint.h>
#include <string.h>
#include <sys/syslimits.h>

static napi_value verify(napi_env env, napi_callback_info callback) {
    const char *hex = "0123456789abcdef";
    size_t argc = 2, path_len = 0, role_len = 0;
    napi_value args[2], result;
    char path[PATH_MAX], role[8], digest[40];
    CFStringRef rule = NULL;
    CFURLRef url = NULL;
    SecStaticCodeRef code = NULL;
    SecRequirementRef requirement = NULL;
    CFDictionaryRef info = NULL;
    napi_get_null(env, &result);
    if (napi_get_cb_info(env, callback, &argc, args, NULL, NULL) != napi_ok || argc != 2 ||
        napi_get_value_string_utf8(env, args[0], path, sizeof(path), &path_len) != napi_ok ||
        path_len == 0 || path_len == sizeof(path) - 1 || strlen(path) != path_len || path[0] != '/' ||
        napi_get_value_string_utf8(env, args[1], role, sizeof(role), &role_len) != napi_ok ||
        strlen(role) != role_len) return result;

    CFStringRef identifier;
    if (strcmp(role, "hub") == 0) identifier = CFSTR("app.mexerica.mac.rc2.hub");
    else if (strcmp(role, "helper") == 0) identifier = CFSTR("app.mexerica.mac.rc2.helper");
    else return result;

    rule = CFStringCreateWithFormat(NULL, NULL, CFSTR(
        "identifier \"%@\" and anchor apple generic and "
        "certificate 1[field.1.2.840.113635.100.6.2.6] exists and "
        "certificate leaf[field.1.2.840.113635.100.6.1.13] exists and "
        "certificate leaf[subject.OU] = \"XYTFAFLUK9\""), identifier);
    url = CFURLCreateFromFileSystemRepresentation(NULL, (const UInt8 *)path, (CFIndex)path_len, false);
    if (!rule || !url ||
        SecRequirementCreateWithString(rule, kSecCSDefaultFlags, &requirement) != errSecSuccess ||
        SecStaticCodeCreateWithPath(url, kSecCSDefaultFlags, &code) != errSecSuccess ||
        SecStaticCodeCheckValidity(code, kSecCSStrictValidate | kSecCSCheckAllArchitectures | kSecCSNoNetworkAccess, requirement) != errSecSuccess ||
        SecCodeCopySigningInformation(code, kSecCSSigningInformation, &info) != errSecSuccess || !info) goto done;

    CFDataRef cdhash = CFDictionaryGetValue(info, kSecCodeInfoUnique);
    CFStringRef team = CFDictionaryGetValue(info, kSecCodeInfoTeamIdentifier);
    CFNumberRef flags = CFDictionaryGetValue(info, kSecCodeInfoFlags);
    CFDictionaryRef entitlements = CFDictionaryGetValue(info, kSecCodeInfoEntitlementsDict);
    int64_t signature_flags = 0;
    if (!cdhash || CFGetTypeID(cdhash) != CFDataGetTypeID() || CFDataGetLength(cdhash) != 20 ||
        !team || !CFEqual(team, CFSTR("XYTFAFLUK9")) ||
        !flags || CFGetTypeID(flags) != CFNumberGetTypeID() ||
        !CFNumberGetValue(flags, kCFNumberSInt64Type, &signature_flags) ||
        !(signature_flags & kSecCodeSignatureRuntime) || (signature_flags & kSecCodeSignatureAdhoc) ||
        (entitlements && (CFGetTypeID(entitlements) != CFDictionaryGetTypeID() || CFDictionaryGetCount(entitlements)))) goto done;

    const UInt8 *bytes = CFDataGetBytePtr(cdhash);
    for (size_t i = 0; i < 20; i++) {
        digest[i * 2] = hex[bytes[i] >> 4];
        digest[i * 2 + 1] = hex[bytes[i] & 15];
    }
    napi_create_string_utf8(env, digest, sizeof(digest), &result);

done:
    if (info) CFRelease(info);
    if (code) CFRelease(code);
    if (requirement) CFRelease(requirement);
    if (url) CFRelease(url);
    if (rule) CFRelease(rule);
    return result;
}

NAPI_MODULE_INIT() {
    napi_value checker;
    if (napi_create_function(env, "verify", NAPI_AUTO_LENGTH, verify, NULL, &checker) != napi_ok ||
        napi_set_named_property(env, exports, "verify", checker) != napi_ok) return NULL;
    return exports;
}
