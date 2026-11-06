use std::{env, path::Path};

fn main() {
    let dir = env::var_os("OUT_DIR").unwrap();
    bindings(&dir);
    build(&dir);
}

fn build(out_dir: impl AsRef<Path>) {
    let mut libraw = cc::Build::new();
    let compiler = libraw.get_compiler();
    if compiler.is_like_msvc() {
        panic!("MSVC is not supported");
    }

    libraw.cpp(true);
    libraw.include("LibRaw/");

    libraw.file("LibRaw/src/decoders/canon_600.cpp");
    libraw.file("LibRaw/src/decoders/crx.cpp");
    libraw.file("LibRaw/src/decoders/decoders_dcraw.cpp");
    libraw.file("LibRaw/src/decoders/decoders_libraw.cpp");
    libraw.file("LibRaw/src/decoders/decoders_libraw_dcrdefs.cpp");
    libraw.file("LibRaw/src/decoders/dng.cpp");
    libraw.file("LibRaw/src/decoders/fp_dng.cpp");
    libraw.file("LibRaw/src/decoders/fuji_compressed.cpp");
    libraw.file("LibRaw/src/decoders/generic.cpp");
    libraw.file("LibRaw/src/decoders/kodak_decoders.cpp");
    libraw.file("LibRaw/src/decoders/load_mfbacks.cpp");
    // New in LibRaw 0.22 relative to the 0.21.3 tree rsraw-sys 0.1.1 shipped.
    libraw.file("LibRaw/src/decoders/olympus14.cpp");
    libraw.file("LibRaw/src/decoders/pana8.cpp");
    libraw.file("LibRaw/src/decoders/smal.cpp");
    libraw.file("LibRaw/src/decoders/sonycc.cpp");
    libraw.file("LibRaw/src/decoders/unpack.cpp");
    libraw.file("LibRaw/src/decoders/unpack_thumb.cpp");
    libraw.file("LibRaw/src/decompressors/losslessjpeg.cpp");
    libraw.file("LibRaw/src/demosaic/aahd_demosaic.cpp");
    libraw.file("LibRaw/src/demosaic/ahd_demosaic.cpp");
    libraw.file("LibRaw/src/demosaic/dcb_demosaic.cpp");
    libraw.file("LibRaw/src/demosaic/dht_demosaic.cpp");
    libraw.file("LibRaw/src/demosaic/misc_demosaic.cpp");
    libraw.file("LibRaw/src/demosaic/xtrans_demosaic.cpp");
    libraw.file("LibRaw/src/integration/dngsdk_glue.cpp");
    libraw.file("LibRaw/src/integration/rawspeed_glue.cpp");
    libraw.file("LibRaw/src/metadata/adobepano.cpp");
    libraw.file("LibRaw/src/metadata/canon.cpp");
    libraw.file("LibRaw/src/metadata/ciff.cpp");
    libraw.file("LibRaw/src/metadata/cr3_parser.cpp");
    libraw.file("LibRaw/src/metadata/epson.cpp");
    libraw.file("LibRaw/src/metadata/exif_gps.cpp");
    libraw.file("LibRaw/src/metadata/fuji.cpp");
    libraw.file("LibRaw/src/metadata/hasselblad_model.cpp");
    libraw.file("LibRaw/src/metadata/identify.cpp");
    libraw.file("LibRaw/src/metadata/identify_tools.cpp");
    libraw.file("LibRaw/src/metadata/kodak.cpp");
    libraw.file("LibRaw/src/metadata/leica.cpp");
    libraw.file("LibRaw/src/metadata/makernotes.cpp");
    libraw.file("LibRaw/src/metadata/mediumformat.cpp");
    libraw.file("LibRaw/src/metadata/minolta.cpp");
    libraw.file("LibRaw/src/metadata/misc_parsers.cpp");
    libraw.file("LibRaw/src/metadata/nikon.cpp");
    libraw.file("LibRaw/src/metadata/normalize_model.cpp");
    libraw.file("LibRaw/src/metadata/olympus.cpp");
    libraw.file("LibRaw/src/metadata/p1.cpp");
    libraw.file("LibRaw/src/metadata/pentax.cpp");
    libraw.file("LibRaw/src/metadata/samsung.cpp");
    libraw.file("LibRaw/src/metadata/sony.cpp");
    libraw.file("LibRaw/src/metadata/tiff.cpp");
    libraw.file("LibRaw/src/postprocessing/aspect_ratio.cpp");
    libraw.file("LibRaw/src/postprocessing/dcraw_process.cpp");
    libraw.file("LibRaw/src/postprocessing/mem_image.cpp");
    libraw.file("LibRaw/src/postprocessing/postprocessing_aux.cpp");
    //libraw.file("LibRaw/src/postprocessing/postprocessing_ph.cpp");
    libraw.file("LibRaw/src/postprocessing/postprocessing_utils.cpp");
    libraw.file("LibRaw/src/postprocessing/postprocessing_utils_dcrdefs.cpp");
    libraw.file("LibRaw/src/preprocessing/ext_preprocess.cpp");
    //libraw.file("LibRaw/src/preprocessing/preprocessing_ph.cpp");
    libraw.file("LibRaw/src/preprocessing/raw2image.cpp");
    libraw.file("LibRaw/src/preprocessing/subtract_black.cpp");
    libraw.file("LibRaw/src/tables/cameralist.cpp");
    libraw.file("LibRaw/src/tables/colorconst.cpp");
    libraw.file("LibRaw/src/tables/colordata.cpp");
    libraw.file("LibRaw/src/tables/wblists.cpp");
    libraw.file("LibRaw/src/utils/curves.cpp");
    libraw.file("LibRaw/src/utils/decoder_info.cpp");
    libraw.file("LibRaw/src/utils/init_close_utils.cpp");
    libraw.file("LibRaw/src/utils/open.cpp");
    libraw.file("LibRaw/src/utils/phaseone_processing.cpp");
    libraw.file("LibRaw/src/utils/read_utils.cpp");
    libraw.file("LibRaw/src/utils/thumb_utils.cpp");
    libraw.file("LibRaw/src/utils/utils_dcraw.cpp");
    libraw.file("LibRaw/src/utils/utils_libraw.cpp");
    libraw.file("LibRaw/src/write/apply_profile.cpp");
    libraw.file("LibRaw/src/write/file_write.cpp");
    libraw.file("LibRaw/src/write/tiff_writer.cpp");
    //libraw.file("LibRaw/src/write/write_ph.cpp");
    libraw.file("LibRaw/src/x3f/x3f_parse_process.cpp");
    libraw.file("LibRaw/src/x3f/x3f_utils_patched.cpp");
    libraw.file("LibRaw/src/libraw_c_api.cpp");
    // libraw.file("LibRaw/src/libraw_cxx.cpp");
    libraw.file("LibRaw/src/libraw_datastream.cpp");

    libraw.warnings(false);
    libraw.extra_warnings(false);
    // do I really have to supress all of these?
    libraw.flag_if_supported("-Wno-format-truncation");
    libraw.flag_if_supported("-Wno-unused-result");
    libraw.flag_if_supported("-Wno-format-overflow");

    // thread safety
    libraw.flag("-pthread");
    libraw.compile("raw");

    println!(
        "cargo:rustc-link-search=native={}",
        out_dir.as_ref().join("lib").display()
    );
    println!("cargo:rustc-link-lib=static=raw");
}

fn bindings(out_dir: impl AsRef<Path>) {
    let path = out_dir.as_ref().join("bindings.rs");
    let host = env::var("HOST").expect("HOST is not set");
    let target = env::var("TARGET").expect("TARGET is not set");

    // Only native builds are supported. The old fallback reused the upstream
    // 0.1.1 pregenerated bindings (LibRaw 0.21.3); paired with the vendored
    // 0.22.2 tree they describe the wrong struct layouts.
    if host != target {
        panic!(
            "rsraw-sys: only native builds (HOST == TARGET) are supported; \
             for cross compilation regenerate bindings from this tree's \
             LibRaw 0.22.2 headers first"
        );
    }

    let bindings = bindgen::Builder::default()
        .header("LibRaw/libraw/libraw.h")
        .use_core()
        .ctypes_prefix("libc")
        .generate_comments(true)
        .parse_callbacks(Box::new(bindgen::CargoCallbacks::new()))
        .layout_tests(false)
        // API improvements
        .derive_eq(true)
        .no_partialeq("__darwin_pthread_handler_rec")
        .no_partialeq("_IO_FILE") // This is the unnamed struct with _close, _read, etc.
        .no_partialeq("_IO_cookie_io_functions_t")
        .no_partialeq("sigvec")
        .no_partialeq("libraw_callbacks_t")
        .no_partialeq("__sFILE")
        .size_t_is_usize(true)
        // these are never part of the API
        .blocklist_function("_.*")
        // consts creating duplications
        .blocklist_item("FP_NAN")
        .blocklist_item("FP_INFINITE")
        .blocklist_item("FP_ZERO")
        .blocklist_item("FP_SUBNORMAL")
        .blocklist_item("FP_NORMAL")
        // Rust doesn't support long double, and bindgen can't skip it
        // https://github.com/rust-lang/rust-bindgen/issues/1549
        .blocklist_function("acoshl")
        .blocklist_function("acosl")
        .blocklist_function("asinhl")
        .blocklist_function("asinl")
        .blocklist_function("atan2l")
        .blocklist_function("atanhl")
        .blocklist_function("atanl")
        .blocklist_function("cbrtl")
        .blocklist_function("ceill")
        .blocklist_function("copysignl")
        .blocklist_function("coshl")
        .blocklist_function("cosl")
        .blocklist_function("dreml")
        .blocklist_function("ecvt_r")
        .blocklist_function("erfcl")
        .blocklist_function("erfl")
        .blocklist_function("exp2l")
        .blocklist_function("expl")
        .blocklist_function("expm1l")
        .blocklist_function("fabsl")
        .blocklist_function("fcvt_r")
        .blocklist_function("fdiml")
        .blocklist_function("finitel")
        .blocklist_function("floorl")
        .blocklist_function("fmal")
        .blocklist_function("fmaxl")
        .blocklist_function("fminl")
        .blocklist_function("fmodl")
        .blocklist_function("frexpl")
        .blocklist_function("gammal")
        .blocklist_function("hypotl")
        .blocklist_function("ilogbl")
        .blocklist_function("isinfl")
        .blocklist_function("isnanl")
        .blocklist_function("j0l")
        .blocklist_function("j1l")
        .blocklist_function("jnl")
        .blocklist_function("ldexpl")
        .blocklist_function("lgammal")
        .blocklist_function("lgammal_r")
        .blocklist_function("llrintl")
        .blocklist_function("llroundl")
        .blocklist_function("log10l")
        .blocklist_function("log1pl")
        .blocklist_function("log2l")
        .blocklist_function("logbl")
        .blocklist_function("logl")
        .blocklist_function("lrintl")
        .blocklist_function("lroundl")
        .blocklist_function("modfl")
        .blocklist_function("nanl")
        .blocklist_function("nearbyintl")
        .blocklist_function("nextafterl")
        .blocklist_function("nexttoward")
        .blocklist_function("nexttowardf")
        .blocklist_function("nexttowardl")
        .blocklist_function("powl")
        .blocklist_function("qecvt")
        .blocklist_function("qecvt_r")
        .blocklist_function("qfcvt")
        .blocklist_function("qfcvt_r")
        .blocklist_function("qgcvt")
        .blocklist_function("remainderl")
        .blocklist_function("remquol")
        .blocklist_function("rintl")
        .blocklist_function("roundl")
        .blocklist_function("scalbl")
        .blocklist_function("scalblnl")
        .blocklist_function("scalbnl")
        .blocklist_function("significandl")
        .blocklist_function("sinhl")
        .blocklist_function("sinl")
        .blocklist_function("sqrtl")
        .blocklist_function("strtold")
        .blocklist_function("tanhl")
        .blocklist_function("tanl")
        .blocklist_function("tgammal")
        .blocklist_function("truncl")
        .blocklist_function("y0l")
        .blocklist_function("y1l")
        .blocklist_function("ynl")
        .generate()
        .expect("Unable to generate bindings");

    bindings
        .write_to_file(path)
        .expect("Couldn't write bindings!");
}
