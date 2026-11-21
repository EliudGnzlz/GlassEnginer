# Bug D: el CRT de MSVC se enlaza estatico en todo el workspace
# (.cargo/config.toml, target-feature=+crt-static). La libreria C++ de
# manifold (manifold-csg-sys, MeshForge) se compila con CMake, que por
# defecto usa el CRT dinamico (/MD): sin esto sus objetos importan los
# simbolos __imp_* del UCRT dinamico y el enlazado de glass_app en
# Windows falla con LNK2019/LNK1120 (Release 44843024309).
#
# Se entrega por la variable de entorno CMAKE_TOOLCHAIN_FILE, que CMake
# inicializa desde el entorno al configurar (verificado en la VM); en
# Linux es inocua: la variable solo afecta al generador de MSVC.
set(CMAKE_MSVC_RUNTIME_LIBRARY "MultiThreaded")
