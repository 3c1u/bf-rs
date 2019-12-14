use crate::parser::BfAST;
use crate::{Error, Result};

use inkwell::builder::Builder;
use inkwell::context::Context;
use inkwell::execution_engine::{ExecutionEngine, JitFunction};
use inkwell::module::Module;
use inkwell::AddressSpace;
use inkwell::IntPredicate;
use inkwell::OptimizationLevel;

// use crate::ice;

use inkwell::values::{FunctionValue, IntValue, PointerValue};

use std::io::{Read, Write};

pub type BfBootstrap = unsafe extern "C" fn(
    unsafe extern "C" fn() -> u8,
    unsafe extern "C" fn(c: u8),
    unsafe extern "C" fn(ptr: *mut u8, value: u8, len: u64),
);

pub struct Codegen<'c> {
    context: &'c Context,
    module: Module<'c>,
    builder: Builder<'c>,
    execution_engine: ExecutionEngine<'c>,
}

extern "C" fn bfrs_get_char() -> u8 {
    let mut input = std::io::stdin();
    let mut buf = [0u8];
    input.read(&mut buf).unwrap();
    buf[0]
}

extern "C" fn bfrs_print_char(c: u8) {
    let mut out = std::io::stdout();
    out.write(&[c]).unwrap();
    out.flush().unwrap();
}

unsafe extern "C" fn bfrs_memset(ptr: *mut u8, value: u8, len: u64) {
    std::ptr::write_bytes(ptr, value, len as usize);
}

impl<'c> Codegen<'c> {
    pub fn new(context: &'c Context, optimized: bool) -> Result<Self> {
        let module = context.create_module("bfrs");

        let execution_engine = module
            .create_jit_execution_engine(if optimized {
                OptimizationLevel::Aggressive
            } else {
                OptimizationLevel::None
            })
            .map_err(|_| Error::Ice("failed to create execution engine".into()))?;
        let builder = context.create_builder();

        Ok(Self {
            context,
            module,
            execution_engine,
            builder,
        })
    }

    pub fn run(&self, ast: &[BfAST]) -> Result<()> {
        // 実行環境の構築
        let get_char_type = self
            .context
            .i8_type()
            .fn_type(&[], false)
            .ptr_type(AddressSpace::Global);

        let put_char_type = self
            .context
            .void_type()
            .fn_type(&[self.context.i8_type().into()], false)
            .ptr_type(AddressSpace::Global);

        let memset_type = self
            .context
            .void_type()
            .fn_type(
                &[
                    self.context
                        .i8_type()
                        .ptr_type(AddressSpace::Generic)
                        .into(),
                    self.context.i8_type().into(),
                    self.context.i64_type().into(),
                ],
                false,
            )
            .ptr_type(AddressSpace::Global);

        let fn_type = self.context.void_type().fn_type(
            &[
                get_char_type.into(),
                put_char_type.into(),
                memset_type.into(),
            ],
            false,
        );

        let func = self.module.add_function("bfrs_lang_start", fn_type, None);

        let basic_block = self.context.append_basic_block(func, "entry");

        self.builder.position_at_end(&basic_block);

        let get_char = func.get_nth_param(0).unwrap().into_pointer_value();
        let put_char = func.get_nth_param(1).unwrap().into_pointer_value();
        let memset = func.get_nth_param(2).unwrap().into_pointer_value();

        let value_table = self.context.i8_type().array_type(10000);
        let value_table = self.builder.build_alloca(value_table, "");
        let counter = self.builder.build_alloca(self.context.i64_type(), "");

        self.builder
            .build_store(counter, self.context.i64_type().const_int(0, false));

        self.builder.build_call(
            memset,
            &[
                value_table.into(),
                self.context.i8_type().const_int(0, false).into(),
                self.context.i64_type().const_int(10000, false).into(),
            ],
            "",
        );

        for op in ast {
            self.build_operation(func, (get_char, put_char), op, value_table, counter)?;
        }

        self.builder.build_return(None);

        print!("building...");
        std::io::stdout().flush().unwrap();

        let entry: JitFunction<BfBootstrap> =
            unsafe { self.execution_engine.get_function("bfrs_lang_start") }.unwrap();

        print!("\u{001b}[2K\r");
        std::io::stdout().flush().unwrap();

        unsafe {
            entry.call(bfrs_get_char, bfrs_print_char, bfrs_memset);
        }

        Ok(())
    }

    fn build_operation(
        &self,
        function: FunctionValue<'c>,
        env: (PointerValue<'c>, PointerValue<'c>),
        operation: &BfAST,
        value_table: PointerValue<'c>,
        counter: PointerValue<'c>,
    ) -> Result<()> {
        let cur = self.get_current(value_table, counter);

        match operation {
            BfAST::LoopBlock(v) => {
                // 特殊パターンの高速化
                if v.len() == 0 {
                    return Ok(());
                } else if v.len() == 1 {
                    if let BfAST::SubOp(k) = v[0] {
                        if k == 1 {
                            self.set_current(
                                value_table,
                                counter,
                                self.context.i8_type().const_int(0 as u64, false),
                            );
                        } else {
                            let res = self.builder.build_int_unsigned_rem(
                                self.get_current(value_table, counter),
                                self.context.i8_type().const_int(k as u64, false),
                                "",
                            );
                            self.set_current(value_table, counter, res);
                        }

                        return Ok(());
                    }
                } else if v.len() == 3 {
                    if let &[BfAST::AddPtr(j), BfAST::AddOp(k), BfAST::SubPtr(l)] = &v[0..3] {
                        if j == l {
                            let rhs = self.get_current(value_table, counter);

                            let lh_pos = self.builder.build_int_add(
                                self.builder.build_load(counter, "").into_int_value(),
                                self.context.i64_type().const_int(j as u64, false),
                                "",
                            );

                            let lhs_ref = unsafe {
                                self.builder.build_in_bounds_gep(
                                    value_table,
                                    &[self.context.i64_type().const_int(0, false), lh_pos],
                                    "",
                                )
                            };

                            let orig = self.builder.build_load(lhs_ref, "").into_int_value();
                            let lhs = self.context.i8_type().const_int(k as u64, false);

                            let res = self.builder.build_int_nuw_add(
                                orig,
                                self.builder.build_int_nuw_mul(lhs, rhs, ""),
                                "",
                            );

                            self.builder.build_store(lhs_ref, res);
                            self.set_current(value_table, counter, res);

                            return Ok(());
                        }
                    } else if let &[BfAST::SubPtr(j), BfAST::AddOp(k), BfAST::AddPtr(l)] = &v[0..3]
                    {
                        if j == l {
                            let rhs = self.get_current(value_table, counter);

                            let lh_pos = self.builder.build_int_nuw_add(
                                self.builder.build_load(counter, "").into_int_value(),
                                self.context.i64_type().const_int(j as u64, false),
                                "",
                            );

                            let lhs_ref = unsafe {
                                self.builder.build_in_bounds_gep(
                                    value_table,
                                    &[self.context.i64_type().const_int(0, false), lh_pos],
                                    "",
                                )
                            };

                            let orig = self.builder.build_load(lhs_ref, "").into_int_value();
                            let lhs = self.context.i8_type().const_int(k as u64, false);

                            let res = self.builder.build_int_nuw_add(
                                orig,
                                self.builder.build_int_nuw_mul(lhs, rhs, ""),
                                "",
                            );

                            self.builder.build_store(lhs_ref, res);
                            self.set_current(value_table, counter, res);

                            return Ok(());
                        }
                    }
                }

                let loop_head = self.context.append_basic_block(function, "");
                let loop_body = self.context.append_basic_block(function, "");
                let loop_end = self.context.append_basic_block(function, "");

                self.builder.build_unconditional_branch(&loop_head);

                self.builder.position_at_end(&loop_head);

                self.builder.build_conditional_branch(
                    self.builder.build_int_compare(
                        IntPredicate::EQ,
                        self.get_current(value_table, counter),
                        self.context.i8_type().const_int(0, false),
                        "",
                    ),
                    &loop_end,
                    &loop_body,
                );

                self.builder.position_at_end(&loop_body);

                for i in v {
                    self.build_operation(function, env, i, value_table, counter)?;
                }

                self.builder.build_unconditional_branch(&loop_head);

                self.builder.position_at_end(&loop_end);
            }
            BfAST::AddOp(k) => {
                let cur = self.builder.build_int_add(
                    cur,
                    self.context.i8_type().const_int(*k as u64, false),
                    "",
                );
                self.set_current(value_table, counter, cur);
            }
            BfAST::SubOp(k) => {
                let cur = self.builder.build_int_sub(
                    cur,
                    self.context.i8_type().const_int(*k as u64, false),
                    "",
                );
                self.set_current(value_table, counter, cur);
            }
            BfAST::AddPtr(k) => {
                let counter_v = self.builder.build_load(counter, "").into_int_value();
                let counter_incr = self.builder.build_int_add(
                    counter_v,
                    self.context.i64_type().const_int(*k as u64, false),
                    "",
                );
                self.builder.build_store(counter, counter_incr);
            }
            BfAST::SubPtr(k) => {
                let counter_v = self.builder.build_load(counter, "").into_int_value();
                let counter_incr = self.builder.build_int_sub(
                    counter_v,
                    self.context.i64_type().const_int(*k as u64, false),
                    "",
                );
                self.builder.build_store(counter, counter_incr);
            }
            BfAST::PutChar => {
                self.builder.build_call(env.1, &[cur.into()], "");
            }
            BfAST::GetChar => {
                let res = self
                    .builder
                    .build_call(env.0, &[], "")
                    .try_as_basic_value()
                    .left()
                    .unwrap()
                    .into_int_value();

                self.set_current(value_table, counter, res);
            }
        }

        Ok(())
    }

    pub fn get_current(
        &self,
        value_table: PointerValue<'c>,
        counter: PointerValue<'c>,
    ) -> IntValue<'c> {
        let counter = self.builder.build_load(counter, "");
        let value = unsafe {
            self.builder.build_in_bounds_gep(
                value_table,
                &[
                    self.context.i64_type().const_int(0, false),
                    counter.into_int_value(),
                ],
                "",
            )
        };

        self.builder.build_load(value, "").into_int_value()
    }

    pub fn set_current(
        &self,
        value_table: PointerValue<'c>,
        counter: PointerValue<'c>,
        value: IntValue<'c>,
    ) {
        let counter = self.builder.build_load(counter, "");
        let ref_val = unsafe {
            self.builder.build_in_bounds_gep(
                value_table,
                &[
                    self.context.i64_type().const_int(0, false),
                    counter.into_int_value(),
                ],
                "",
            )
        };

        self.builder.build_store(ref_val, value);
    }
}
