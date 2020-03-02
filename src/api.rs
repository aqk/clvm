use super::eval::Reduction;
use super::eval::{run_program, EvalErr, FApplyFallback, PostEval, PreEval};
use super::f_table::make_f_lookup;
use super::serialize::{node_from_stream, node_to_stream};
use super::sexp::Node;
use pyo3::prelude::*;
use pyo3::types::PyBytes;
use pyo3::wrap_pyfunction;
use pyo3::PyObject;
use std::io::Cursor;
use std::io::{Seek, SeekFrom, Write};

impl From<PyErr> for EvalErr {
    fn from(_err: PyErr) -> Self {
        EvalErr(Node::blob("PyErr"), "bad type from python call".to_string())
    }
}

fn node_from_bytes(b: &[u8]) -> std::io::Result<Node> {
    let mut buffer = Cursor::new(Vec::new());
    buffer.write_all(&b)?;
    buffer.seek(SeekFrom::Start(0))?;
    node_from_stream(&mut buffer)
}

fn node_to_bytes(node: &Node) -> std::io::Result<Vec<u8>> {
    let mut buffer = Cursor::new(Vec::new());

    node_to_stream(node, &mut buffer)?;
    let vec = buffer.into_inner();
    Ok(vec)
}

fn wrap_py_post_eval(py: Python<'static>, py_post_eval: PyObject) -> PostEval {
    if py_post_eval.is_none() {
        None
    } else {
        Some(Box::new(move |sexp| -> () {
            let _r = py_post_eval.call1(py, (node_to_bytes(&sexp).unwrap(),));
        }))
    }
}

#[pyfunction]
fn do_eval(
    py: Python<'static>,
    form_u8: &PyBytes,
    env_u8: &PyBytes,
    apply3: PyObject,
    py_pre_eval: PyObject,
    op_quote: u8,
    op_args: u8,
) -> PyResult<(String, Vec<u8>, u32)> {
    let sexp = node_from_bytes(form_u8.as_bytes())?;
    let env = node_from_bytes(env_u8.as_bytes())?;
    let f_table = make_f_lookup();
    let py_apply: FApplyFallback = Box::new(
        move |_eval_context, sexp, args| -> Result<Reduction, EvalErr> {
            let byte_vec: Vec<u8> = apply3
                .call1(py, (node_to_bytes(&sexp)?, node_to_bytes(&args)?))?
                .extract(py)?;
            let bytes: &[u8] = &byte_vec;
            Ok(Reduction(node_from_bytes(bytes)?, 1000))
        },
    );

    let pre_eval: PreEval = {
        if py_pre_eval.is_none() {
            None
        } else {
            Some(Box::new(
                move |sexp, args, current_cost, max_cost| -> Result<PostEval, EvalErr> {
                    let py_post_eval: PyObject = py_pre_eval
                        .call1(
                            py,
                            (
                                node_to_bytes(&sexp)?,
                                node_to_bytes(&args)?,
                                current_cost,
                                max_cost,
                            ),
                        )?
                        .extract(py)?;
                    Ok(wrap_py_post_eval(py, py_post_eval))
                },
            ))
        }
    };
    let r = run_program(
        &sexp, &env, 0, 100_000, &f_table, py_apply, pre_eval, op_quote, op_args,
    );
    match r {
        Ok(Reduction(node, cycles)) => Ok(("".into(), node_to_bytes(&node)?, cycles)),
        Err(EvalErr(node, err)) => Ok((err, node_to_bytes(&node)?, 0)),
    }
}

/// This module is a python module implemented in Rust.
#[pymodule]
fn clvmr(_py: Python, m: &PyModule) -> PyResult<()> {
    m.add_wrapped(wrap_pyfunction!(do_eval))?;
    Ok(())
}
