#![deny(
    missing_debug_implementations,
    trivial_numeric_casts,
    unstable_features,
    unused_import_braces,
    unused_qualifications
)]

extern crate clap;
extern crate notify;
extern crate rouille;
extern crate tempdir;
extern crate reqwest;
extern crate pbr;
extern crate app_dirs;
extern crate libflate;
extern crate tar;
extern crate sha1;
extern crate sha2;
extern crate digest;
extern crate toml;
extern crate serde;
#[macro_use]
extern crate serde_derive;
extern crate cargo_shim;
extern crate handlebars;
extern crate unicode_categories;
extern crate ordermap;

extern crate parity_wasm;
#[macro_use]
extern crate log;
extern crate rustc_demangle;

use std::process::exit;
use std::env;

use clap::{
    Arg,
    App,
    AppSettings,
    SubCommand
};

use cargo_shim::*;

#[macro_use]
mod utils;
mod config;
mod package;
mod build;
mod error;
mod wasm;
mod wasm_gc;
mod wasm_inline_js;
mod wasm_export_main;
mod wasm_export_table;
mod wasm_hook_grow;
mod wasm_runtime;
mod wasm_context;
mod wasm_intrinsics;
mod emscripten;
mod cmd_build;
mod cmd_start;
mod cmd_test;

fn add_shared_build_params< 'a, 'b >( app: App< 'a, 'b > ) -> App< 'a, 'b > {
    return app
        .arg(
            Arg::with_name( "package" )
                .short( "p" )
                .long( "package" )
                .help( "Package to build" )
                .value_name( "NAME" )
                .takes_value( true )
        )
        .arg(
            Arg::with_name( "use-system-emscripten" )
                .long( "use-system-emscripten" )
                .help( "Won't try to download Emscripten; will always use the system one" )
        )
        .arg(
            Arg::with_name( "release" )
                .long( "release" )
                .help( "Build artifacts in release mode, with optimizations" )
        )
        .arg(
            Arg::with_name( "target-asmjs-emscripten" )
                .long( "target-asmjs-emscripten" )
                .help( "Generate asmjs through Emscripten (default)" )
                .overrides_with_all( &["target-webasm-emscripten", "target-webasm"] )
        )
        .arg(
            Arg::with_name( "target-webasm-emscripten" )
                .long( "target-webasm-emscripten" )
                .help( "Generate webasm through Emscripten" )
                .overrides_with_all( &["target-asmjs-emscripten", "target-webasm"] )
        )
        .arg(
            Arg::with_name( "target-webasm" )
                .long( "target-webasm" )
                .help( "Generates webasm through Rust's native backend (HIGHLY EXPERIMENTAL!)" )
                .overrides_with_all( &["target-asmjs-emscripten", "target-webasm-emscripten"] )
        );
}

fn main() {
    let args = {
        // To allow running both as 'cargo-web' and as 'cargo web'.
        let mut args = env::args();
        let mut filtered_args = Vec::new();
        filtered_args.push( args.next().unwrap() );

        match args.next() {
            None => {},
            #[cfg(any(unix))]
            Some( ref arg ) if filtered_args[ 0 ].ends_with( "cargo-web" ) && arg == "web" => {},
            #[cfg(any(windows))]
            Some( ref arg ) if filtered_args[ 0 ].ends_with( "cargo-web.exe" ) && arg == "web" => {},
            Some( arg ) => filtered_args.push( arg )
        }

        filtered_args.extend( args );
        filtered_args
    };

    let mut build_subcommand =
        SubCommand::with_name( "build" )
            .about( "Compile a local package and all of its dependencies" )
            .arg(
                Arg::with_name( "lib" )
                    .long( "lib" )
                    .help( "Build only this package's library" )
            )
            .arg(
                Arg::with_name( "bin" )
                    .long( "bin" )
                    .help( "Build only the specified binary" )
                    .value_name( "NAME" )
                    .takes_value( true )
            )
            .arg(
                Arg::with_name( "example" )
                    .long( "example" )
                    .help( "Build only the specified example" )
                    .value_name( "NAME" )
                    .takes_value( true )
            )
            .arg(
                Arg::with_name( "test" )
                    .long( "test" )
                    .help( "Build only the specified test target" )
                    .value_name( "NAME" )
                    .takes_value( true )
            )
            .arg(
                Arg::with_name( "bench" )
                    .long( "bench" )
                    .help( "Build only the specified benchmark target" )
                    .value_name( "NAME" )
                    .takes_value( true )
            );

    let mut test_subcommand =
        SubCommand::with_name( "test" )
            .about( "Compiles and runs tests" )
            .arg(
                Arg::with_name( "no-run" )
                    .long( "no-run" )
                    .help( "Compile, but don't run tests" )
            )
            .arg(
                Arg::with_name( "nodejs" )
                    .long( "nodejs" )
                    .help( "Uses Node.js to run the tests" )
            )
            .arg(
                Arg::with_name( "passthrough" )
                    .help( "-- followed by anything will pass the arguments to the test runner")
                    .multiple( true )
                    .takes_value( true )
                    .last( true )
            );

    let mut start_subcommand =
        SubCommand::with_name( "start" )
            .about( "Runs an embedded web server serving the built project" )
            .arg(
                Arg::with_name( "bin" )
                    .long( "bin" )
                    .help( "Build only the specified binary" )
                    .value_name( "NAME" )
                    .takes_value( true )
            )
            .arg(
                Arg::with_name( "example" )
                    .long( "example" )
                    .help( "Serves the specified example" )
                    .value_name( "NAME" )
                    .takes_value( true )
            )
            .arg(
                Arg::with_name( "test" )
                    .long( "test" )
                    .help( "Build only the specified test target" )
                    .value_name( "NAME" )
                    .takes_value( true )
            )
            .arg(
                Arg::with_name( "bench" )
                    .long( "bench" )
                    .help( "Build only the specified benchmark target" )
                    .value_name( "NAME" )
                    .takes_value( true )
            ).arg(
                Arg::with_name( "host" )
                    .long( "host" )
                    .help( "Bind the server to this address, default `localhost`")
                    .value_name( "HOST" )
                    .takes_value( true )
            ).arg(
                Arg::with_name( "port" )
                    .long( "port" )
                    .help( "Bind the server to this port, default 8000" )
                    .value_name( "PORT" )
                    .takes_value( true )
            );

    build_subcommand = add_shared_build_params( build_subcommand );
    test_subcommand = add_shared_build_params( test_subcommand );
    start_subcommand = add_shared_build_params( start_subcommand );

    let matches = App::new( "cargo-web" )
        .version( env!( "CARGO_PKG_VERSION" ) )
        .setting( AppSettings::SubcommandRequiredElseHelp )
        .setting( AppSettings::VersionlessSubcommands )
        .subcommand( build_subcommand )
        .subcommand( test_subcommand )
        .subcommand( start_subcommand )
        .get_matches_from( args );

    let project = CargoProject::new( None );
    let result = if let Some( matches ) = matches.subcommand_matches( "build" ) {
        cmd_build::command_build( matches, &project )
    } else if let Some( matches ) = matches.subcommand_matches( "test" ) {
        cmd_test::command_test( matches, &project )
    } else if let Some( matches ) = matches.subcommand_matches( "start" ) {
        cmd_start::command_start( matches, &project )
    } else {
        return;
    };

    match result {
        Ok( _ ) => {},
        Err( error ) => {
            println_err!( "error: {}", error );
            exit( 101 );
        }
    }
}
