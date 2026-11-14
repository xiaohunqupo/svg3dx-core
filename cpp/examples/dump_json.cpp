// Minimal host: load svg3dx_core shared library and print geometry instruction JSON.
// Uses LoadLibrary/dlopen so an MSVC-built DLL works with MinGW without an import lib.
//
// Usage:
//   dump_json <file.svg> [--dll path/to/svg3dx_core.dll] [--model id] [--params '{"R":40}']

#include <cstdint>
#include <cstdlib>
#include <fstream>
#include <iostream>
#include <sstream>
#include <string>
#include <vector>

#ifdef _WIN32
#define WIN32_LEAN_AND_MEAN
#include <windows.h>
#else
#include <dlfcn.h>
#endif

struct Svg3dxResult;

using FnParse = Svg3dxResult *(*)(const uint8_t *, size_t);
using FnParseParams = Svg3dxResult *(*)(const uint8_t *, size_t, const char *, const char *);
using FnOk = bool (*)(const Svg3dxResult *);
using FnJson = const char *(*)(const Svg3dxResult *);
using FnFree = void (*)(Svg3dxResult *);

struct Api {
#ifdef _WIN32
  HMODULE mod = nullptr;
#else
  void *mod = nullptr;
#endif
  FnParse parse = nullptr;
  FnParseParams parse_params = nullptr;
  FnOk ok = nullptr;
  FnJson json = nullptr;
  FnFree free_result = nullptr;

  bool load(const std::string &path) {
#ifdef _WIN32
    mod = LoadLibraryA(path.c_str());
    if (!mod) {
      std::cerr << "LoadLibrary failed: " << path << " (err=" << GetLastError() << ")\n";
      return false;
    }
    parse = reinterpret_cast<FnParse>(GetProcAddress(mod, "svg3dx_parse"));
    parse_params =
        reinterpret_cast<FnParseParams>(GetProcAddress(mod, "svg3dx_parse_with_params"));
    ok = reinterpret_cast<FnOk>(GetProcAddress(mod, "svg3dx_result_ok"));
    json = reinterpret_cast<FnJson>(GetProcAddress(mod, "svg3dx_result_json"));
    free_result = reinterpret_cast<FnFree>(GetProcAddress(mod, "svg3dx_free_result"));
#else
    mod = dlopen(path.c_str(), RTLD_NOW);
    if (!mod) {
      std::cerr << "dlopen failed: " << dlerror() << "\n";
      return false;
    }
    parse = reinterpret_cast<FnParse>(dlsym(mod, "svg3dx_parse"));
    parse_params = reinterpret_cast<FnParseParams>(dlsym(mod, "svg3dx_parse_with_params"));
    ok = reinterpret_cast<FnOk>(dlsym(mod, "svg3dx_result_ok"));
    json = reinterpret_cast<FnJson>(dlsym(mod, "svg3dx_result_json"));
    free_result = reinterpret_cast<FnFree>(dlsym(mod, "svg3dx_free_result"));
#endif
    if (!parse || !ok || !json || !free_result) {
      std::cerr << "missing required exports in " << path << "\n";
      return false;
    }
    return true;
  }

  ~Api() {
    if (!mod) return;
#ifdef _WIN32
    FreeLibrary(mod);
#else
    dlclose(mod);
#endif
  }
};

static void usage() {
  std::cerr
      << "usage: dump_json <file.svg> [--dll path] [--model id] [--params json]\n";
}

int main(int argc, char **argv) {
  if (argc < 2) {
    usage();
    return 1;
  }

  std::string file;
  std::string dll;
  std::string model;
  std::string params = "{}";

  for (int i = 1; i < argc; ++i) {
    std::string a = argv[i];
    if (a == "--dll" && i + 1 < argc) {
      dll = argv[++i];
    } else if (a == "--model" && i + 1 < argc) {
      model = argv[++i];
    } else if (a == "--params" && i + 1 < argc) {
      params = argv[++i];
    } else if (a == "-h" || a == "--help") {
      usage();
      return 0;
    } else if (a[0] == '-') {
      std::cerr << "unknown flag: " << a << "\n";
      return 1;
    } else {
      file = a;
    }
  }
  if (file.empty()) {
    usage();
    return 1;
  }

  if (dll.empty()) {
#ifdef _WIN32
    dll = "svg3dx_core.dll";
#else
    dll = "libsvg3dx_core.so";
#endif
  }

  std::ifstream in(file, std::ios::binary);
  if (!in) {
    std::cerr << "cannot open " << file << "\n";
    return 1;
  }
  std::ostringstream ss;
  ss << in.rdbuf();
  std::string data = ss.str();

  Api api;
  if (!api.load(dll)) return 1;

  Svg3dxResult *result = nullptr;
  if (api.parse_params && (!model.empty() || params != "{}")) {
    result = api.parse_params(
        reinterpret_cast<const uint8_t *>(data.data()),
        data.size(),
        params.c_str(),
        model.empty() ? nullptr : model.c_str());
  } else {
    result = api.parse(reinterpret_cast<const uint8_t *>(data.data()), data.size());
  }

  if (!result) {
    std::cerr << "svg3dx_parse returned null\n";
    return 1;
  }

  const char *json = api.json(result);
  if (!api.ok(result)) {
    std::cerr << "error: " << (json ? json : "(null)") << "\n";
    api.free_result(result);
    return 2;
  }

  std::cout << (json ? json : "{}") << "\n";
  api.free_result(result);
  return 0;
}
