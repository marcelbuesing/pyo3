// Copyright (c) 2017-present PyO3 Project and Contributors
//
// based on Daniel Grunwald's https://github.com/dgrunwald/rust-cpython

//! Functionality for the code generated by the derive backend

use crate::conversion::PyTryFrom;
use crate::err::PyResult;
use crate::exceptions::TypeError;
use crate::ffi;
use crate::init_once;
use crate::types::PyModule;
use crate::types::{PyDict, PyObjectRef, PyString, PyTuple};
use crate::GILPool;
use crate::Python;
use std::ptr;

#[derive(Debug)]
/// Description of a python parameter; used for `parse_args()`.
pub struct ParamDescription<'a> {
    /// The name of the parameter.
    pub name: &'a str,
    /// Whether the parameter is optional.
    pub is_optional: bool,
    /// Whether the parameter is optional.
    pub kw_only: bool,
}

/// Parse argument list
///
/// * fname:  Name of the current function
/// * params: Declared parameters of the function
/// * args:   Positional arguments
/// * kwargs: Keyword arguments
/// * output: Output array that receives the arguments.
///           Must have same length as `params` and must be initialized to `None`.
pub fn parse_fn_args<'p>(
    fname: Option<&str>,
    params: &[ParamDescription],
    args: &'p PyTuple,
    kwargs: Option<&'p PyDict>,
    accept_args: bool,
    accept_kwargs: bool,
    output: &mut [Option<&'p PyObjectRef>],
) -> PyResult<()> {
    let nargs = args.len();
    let nkeywords = kwargs.map_or(0, |d| d.len());
    if !accept_args && !accept_kwargs && (nargs + nkeywords > params.len()) {
        return Err(TypeError::py_err(format!(
            "{}{} takes at most {} argument{} ({} given)",
            fname.unwrap_or("function"),
            if fname.is_some() { "()" } else { "" },
            params.len(),
            if params.len() == 1 { "s" } else { "" },
            nargs + nkeywords
        )));
    }
    let mut used_keywords = 0;
    // Iterate through the parameters and assign values to output:
    for (i, (p, out)) in params.iter().zip(output).enumerate() {
        match kwargs.and_then(|d| d.get_item(p.name)) {
            Some(kwarg) => {
                *out = Some(kwarg);
                used_keywords += 1;
                if i < nargs {
                    return Err(TypeError::py_err(format!(
                        "Argument given by name ('{}') and position ({})",
                        p.name,
                        i + 1
                    )));
                }
            }
            None => {
                if p.kw_only {
                    if !p.is_optional {
                        return Err(TypeError::py_err(format!(
                            "Required argument ('{}') is keyword only argument",
                            p.name
                        )));
                    }
                    *out = None;
                } else if i < nargs {
                    *out = Some(args.get_item(i));
                } else {
                    *out = None;
                    if !p.is_optional {
                        return Err(TypeError::py_err(format!(
                            "Required argument ('{}') (pos {}) not found",
                            p.name,
                            i + 1
                        )));
                    }
                }
            }
        }
    }
    if !accept_kwargs && used_keywords != nkeywords {
        // check for extraneous keyword arguments
        for item in kwargs.unwrap().items().iter() {
            let item = <PyTuple as PyTryFrom>::try_from(item)?;
            let key = <PyString as PyTryFrom>::try_from(item.get_item(0))?.to_string()?;
            if !params.iter().any(|p| p.name == key) {
                return Err(TypeError::py_err(format!(
                    "'{}' is an invalid keyword argument for this function",
                    key
                )));
            }
        }
    }
    Ok(())
}

#[cfg(Py_3)]
#[doc(hidden)]
/// Builds a module (or null) from a user given initializer. Used for `#[pymodule]`.
pub unsafe fn make_module(
    name: &str,
    doc: &str,
    initializer: impl Fn(Python, &PyModule) -> PyResult<()>,
) -> *mut ffi::PyObject {
    use crate::python::IntoPyPointer;

    init_once();

    #[cfg(py_sys_config = "WITH_THREAD")]
    // > Changed in version 3.7: This function is now called by Py_Initialize(), so you don’t have
    // > to call it yourself anymore.
    #[cfg(not(Py_3_7))]
    ffi::PyEval_InitThreads();

    static mut MODULE_DEF: ffi::PyModuleDef = ffi::PyModuleDef_INIT;
    // We can't convert &'static str to *const c_char within a static initializer,
    // so we'll do it here in the module initialization:
    MODULE_DEF.m_name = name.as_ptr() as *const _;

    let module = ffi::PyModule_Create(&mut MODULE_DEF);
    if module.is_null() {
        return module;
    }

    let _pool = GILPool::new();
    let py = Python::assume_gil_acquired();
    let module = match py.from_owned_ptr_or_err::<PyModule>(module) {
        Ok(m) => m,
        Err(e) => {
            e.restore(py);
            return ptr::null_mut();
        }
    };

    module
        .add("__doc__", doc)
        .expect("Failed to add doc for module");
    match initializer(py, module) {
        Ok(_) => module.into_ptr(),
        Err(e) => {
            e.restore(py);
            ptr::null_mut()
        }
    }
}

#[cfg(not(Py_3))]
#[doc(hidden)]
/// Builds a module (or null) from a user given initializer. Used for `#[pymodule]`.
pub unsafe fn make_module(
    name: &str,
    doc: &str,
    initializer: impl Fn(Python, &PyModule) -> PyResult<()>,
) {
    init_once();

    #[cfg(py_sys_config = "WITH_THREAD")]
    ffi::PyEval_InitThreads();

    let _name = name.as_ptr() as *const _;
    let _pool = GILPool::new();
    let py = Python::assume_gil_acquired();
    let _module = ffi::Py_InitModule(_name, ptr::null_mut());
    if _module.is_null() {
        return;
    }

    let _module = match py.from_borrowed_ptr_or_err::<PyModule>(_module) {
        Ok(m) => m,
        Err(e) => {
            e.restore(py);
            return;
        }
    };

    _module
        .add("__doc__", doc)
        .expect("Failed to add doc for module");
    if let Err(e) = initializer(py, _module) {
        e.restore(py)
    }
}
