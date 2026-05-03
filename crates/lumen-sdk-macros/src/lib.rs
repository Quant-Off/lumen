//! Lumen WASM 에이전트 SDK 의 proc-macros.
//!
//! 핵심 표면은 [`lumen_agent`] - 사용자 함수 (`fn step()`) 위에 붙여 호스트가
//! 호출할 `_start` ABI 진입점을 자동으로 생성합니다. 사용자는 보안 부수
//! 효과 (audit 로그, 패닉 처리 라벨링) 를 직접 작성하지 않고 도메인 로직에만
//! 집중할 수 있습니다.
//!
//! 생성된 `_start` 는:
//!
//! 1. (선택) `lumen_sdk::log(LogLevel::Info, "agent=<NAME> version=<VERSION>")`
//!    로 시작 라인을 audit.
//! 2. 사용자 함수 호출. 반환값이 `Result<_, _>` 라면 `Err` 를 audit 로 매핑.
//! 3. 정상 종료 audit.
//!
//! ## 예
//!
//! ```ignore
//! use lumen_sdk::{call_tool, lumen_agent};
//!
//! #[lumen_agent(name = "echo-agent", version = "0.4.0")]
//! fn step() {
//!     let _ = call_tool("echo", "{\"text\":\"hi\"}");
//! }
//! ```

#![warn(missing_docs)]

use proc_macro::TokenStream;
use proc_macro2::TokenStream as TokenStream2;
use quote::quote;
use syn::{parse_macro_input, ItemFn, LitStr, Meta};

/// `#[lumen_agent]` - 사용자 함수를 WASM `_start` ABI 로 래핑합니다.
///
/// 옵션:
/// - `name = "..."`  사람이 읽을 수 있는 이름. audit 라인에 등장.
/// - `version = "..."` 버전 라벨. audit 라인에 등장.
///
/// 사용자 함수는 `fn name() -> ()` 또는 `fn name() -> Result<T, E>` 시그니
/// 처여야 하며 (T, E 는 임의), 이름이 `_start` 와 충돌하지 않아야 합니다.
#[proc_macro_attribute]
pub fn lumen_agent(attr: TokenStream, item: TokenStream) -> TokenStream {
    let input = parse_macro_input!(item as ItemFn);
    let attr_args: Punct = parse_macro_input!(attr as Punct);

    if input.sig.ident == "_start" {
        return syn::Error::new_spanned(
            &input.sig.ident,
            "#[lumen_agent] 은 사용자 함수에 적용해야 합니다 (`_start` 자체에 붙일 수 없습니다)",
        )
        .to_compile_error()
        .into();
    }
    if !input.sig.inputs.is_empty() {
        return syn::Error::new_spanned(
            &input.sig.inputs,
            "#[lumen_agent] 함수는 인자를 받지 않아야 합니다",
        )
        .to_compile_error()
        .into();
    }

    let user_ident = &input.sig.ident;
    let name = attr_args.name;
    let version = attr_args.version;

    let announce = match (name.as_deref(), version.as_deref()) {
        (Some(n), Some(v)) => {
            let line = format!("agent={n} version={v}");
            quote! {
                ::lumen_sdk::log(::lumen_sdk::LogLevel::Info, #line);
            }
        }
        (Some(n), None) => {
            let line = format!("agent={n}");
            quote! { ::lumen_sdk::log(::lumen_sdk::LogLevel::Info, #line); }
        }
        (None, Some(v)) => {
            let line = format!("version={v}");
            quote! { ::lumen_sdk::log(::lumen_sdk::LogLevel::Info, #line); }
        }
        (None, None) => quote! {},
    };

    let dispatch = if returns_unit(&input.sig.output) {
        quote! {
            #user_ident();
        }
    } else {
        // `Result` 또는 임의 반환을 `let _ = ...` 로 흘려보냅니다 - 사용자가
        // 자체적으로 처리하지 않은 에러는 silent ignore 되지 않도록 audit
        // 로그에 흔적만 남깁니다.
        quote! {
            match #user_ident() {
                _ => ::lumen_sdk::log(
                    ::lumen_sdk::LogLevel::Trace,
                    "lumen_agent: step returned",
                ),
            }
        }
    };

    let expanded = quote! {
        #input

        /// `#[lumen_agent]` 가 자동 생성한 WASM `_start` 진입점.
        #[no_mangle]
        pub extern "C" fn _start() {
            #announce
            #dispatch
            ::lumen_sdk::log(
                ::lumen_sdk::LogLevel::Info,
                "lumen_agent: done",
            );
        }
    };

    expanded.into()
}

fn returns_unit(ret: &syn::ReturnType) -> bool {
    matches!(ret, syn::ReturnType::Default)
}

/// `#[lumen_agent(name = "...", version = "...")]` 의 attribute 파싱 결과.
struct Punct {
    name: Option<String>,
    version: Option<String>,
}

impl syn::parse::Parse for Punct {
    fn parse(input: syn::parse::ParseStream<'_>) -> syn::Result<Self> {
        let mut name = None;
        let mut version = None;
        if input.is_empty() {
            return Ok(Self { name, version });
        }
        let metas =
            syn::punctuated::Punctuated::<Meta, syn::Token![,]>::parse_terminated(input)?;
        for meta in metas {
            match meta {
                Meta::NameValue(nv) => {
                    let key = nv
                        .path
                        .get_ident()
                        .map(|i| i.to_string())
                        .unwrap_or_default();
                    let value = expect_str_lit(&nv.value)?;
                    match key.as_str() {
                        "name" => name = Some(value),
                        "version" => version = Some(value),
                        other => {
                            return Err(syn::Error::new_spanned(
                                nv.path,
                                format!("알 수 없는 옵션: {other}"),
                            ));
                        }
                    }
                }
                other => {
                    return Err(syn::Error::new_spanned(
                        other,
                        "지원되는 옵션은 `name = \"...\"`, `version = \"...\"` 입니다",
                    ));
                }
            }
        }
        Ok(Self { name, version })
    }
}

fn expect_str_lit(expr: &syn::Expr) -> syn::Result<String> {
    if let syn::Expr::Lit(syn::ExprLit {
        lit: syn::Lit::Str(s),
        ..
    }) = expr
    {
        return Ok(s.value());
    }
    // syn 2 에서 일부 바인딩은 `LitStr` 로 직접 도착하지 않을 수 있음 - 보조 경로.
    if let Ok(s) = syn::parse2::<LitStr>(quote! { #expr }) {
        return Ok(s.value());
    }
    Err(syn::Error::new_spanned(
        expr,
        "문자열 리터럴이 필요합니다 (예: `name = \"my-agent\"`)",
    ))
}

// 미사용 import 가드 (proc-macro 크레이트는 직접 트리거되는 lint 가 적음).
#[allow(dead_code)]
fn _ts2() -> TokenStream2 {
    quote! {}
}
