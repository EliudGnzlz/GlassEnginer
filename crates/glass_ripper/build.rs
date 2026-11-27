//! Enlaza libstdc++ en Unix: `intel_tex_2` compila un .cpp (ASTC) en su
//! build.rs sin declarar el enlace a la librería estándar de C++; sin
//! esto el enlazado final muere con
//! `undefined symbol: __gxx_personality_v0` (referenciado por
//! `ispc_texcomp_astc.cpp`). En Windows no se emite: MSVC enlaza el
//! runtime de C++ por sus propias directivas (el crate ya distingue
//! -MT/-MD) y no existe libstdc++.
fn main() {
    if cfg!(target_family = "unix") {
        println!("cargo:rustc-link-lib=stdc++");
    }
}
