use crate::parser::BfAST;
use crate::{Error, Result};

use inkwell::builder::Builder;
use inkwell::context::Context;
use inkwell::execution_engine::{ExecutionEngine, JitFunction};
use inkwell::module::Module;
use inkwell::AddressSpace;
use inkwell::IntPredicate;
use inkwell::OptimizationLevel;
use inkwell::basic_block::BasicBlock;
use inkwell::values::{FunctionValue, IntValue, PointerValue};

// use crate::ice;

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

    if input.read(&mut buf).unwrap() == 0 {
        return 0xFF; // EOF
    }

    buf[0]
}

extern "C" fn bfrs_print_char(c: u8) {
    let mut out = std::io::stdout();
    out.write_all(&[c]).unwrap();
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

    fn balanced_loop_optimization(
        &self,
        value_table: PointerValue<'c>,
        counter: PointerValue<'c>,
        v: &[BfAST],
    ) -> Result<bool> {
        // TODO: too dirty; needs to refactor
        
        if let [BfAST::AddPtr(j), BfAST::AddOp(k), BfAST::SubPtr(l), BfAST::SubOp(1)] = v[0..4] {
            if j == l {
                let rhs = self.get_current(value_table, counter);

                let dest_pos = self.builder.build_int_add(
                    self.builder.build_load(counter, "").into_int_value(),
                    self.context.i64_type().const_int(j as u64, false),
                    "",
                );

                let dest_ref = unsafe {
                    self.builder.build_in_bounds_gep(
                        value_table,
                        &[self.context.i64_type().const_int(0, false), dest_pos],
                        "",
                    )
                };

                let dest = self.builder.build_load(dest_ref, "").into_int_value();

                let res = if k != 1 {
                    let k = self.context.i8_type().const_int(k as u64, false);

                    self.builder
                        .build_int_add(dest, self.builder.build_int_mul(k, rhs, ""), "")
                } else {
                    self.builder.build_int_add(dest, rhs, "")
                };

                self.builder.build_store(dest_ref, res);
                self.set_current(
                    value_table,
                    counter,
                    self.context.i8_type().const_int(0, false),
                );

                return Ok(true);
            }
        } else if let [BfAST::SubPtr(j), BfAST::AddOp(k), BfAST::AddPtr(l), BfAST::SubOp(1)] =
            v[0..4]
        {
            if j == l {
                let rhs = self.get_current(value_table, counter);

                let dest_pos = self.builder.build_int_sub(
                    self.builder.build_load(counter, "").into_int_value(),
                    self.context.i64_type().const_int(j as u64, false),
                    "",
                );

                let dest_ref = unsafe {
                    self.builder.build_in_bounds_gep(
                        value_table,
                        &[self.context.i64_type().const_int(0, false), dest_pos],
                        "",
                    )
                };

                let dest = self.builder.build_load(dest_ref, "").into_int_value();

                let res = if k != 1 {
                    let k = self.context.i8_type().const_int(k as u64, false);

                    self.builder
                        .build_int_add(dest, self.builder.build_int_mul(k, rhs, ""), "")
                } else {
                    self.builder.build_int_add(dest, rhs, "")
                };

                self.builder.build_store(dest_ref, res);
                self.set_current(
                    value_table,
                    counter,
                    self.context.i8_type().const_int(0, false),
                );

                return Ok(true);
            }
        } else if let [BfAST::AddPtr(j), BfAST::SubOp(k), BfAST::SubPtr(l), BfAST::SubOp(1)] =
            v[0..4]
        {
            if j == l {
                let rhs = self.get_current(value_table, counter);

                let dest_pos = self.builder.build_int_add(
                    self.builder.build_load(counter, "").into_int_value(),
                    self.context.i64_type().const_int(j as u64, false),
                    "",
                );

                let dest_ref = unsafe {
                    self.builder.build_in_bounds_gep(
                        value_table,
                        &[self.context.i64_type().const_int(0, false), dest_pos],
                        "",
                    )
                };

                let dest = self.builder.build_load(dest_ref, "").into_int_value();

                let res = if k != 1 {
                    let k = self.context.i8_type().const_int(k as u64, false);

                    self.builder
                        .build_int_sub(dest, self.builder.build_int_mul(k, rhs, ""), "")
                } else {
                    self.builder.build_int_sub(dest, rhs, "")
                };

                self.builder.build_store(dest_ref, res);
                self.set_current(
                    value_table,
                    counter,
                    self.context.i8_type().const_int(0, false),
                );

                return Ok(true);
            }
        } else if let [BfAST::SubPtr(j), BfAST::SubOp(k), BfAST::AddPtr(l), BfAST::SubOp(1)] =
            v[0..4]
        {
            if j == l {
                let rhs = self.get_current(value_table, counter);

                let dest_pos = self.builder.build_int_sub(
                    self.builder.build_load(counter, "").into_int_value(),
                    self.context.i64_type().const_int(j as u64, false),
                    "",
                );

                let dest_ref = unsafe {
                    self.builder.build_in_bounds_gep(
                        value_table,
                        &[self.context.i64_type().const_int(0, false), dest_pos],
                        "",
                    )
                };

                let dest = self.builder.build_load(dest_ref, "").into_int_value();

                let res = if k != 1 {
                    let k = self.context.i8_type().const_int(k as u64, false);

                    self.builder
                        .build_int_sub(dest, self.builder.build_int_mul(k, rhs, ""), "")
                } else {
                    self.builder.build_int_sub(dest, rhs, "")
                };

                self.builder.build_store(dest_ref, res);
                self.set_current(
                    value_table,
                    counter,
                    self.context.i8_type().const_int(0, false),
                );

                return Ok(true);
            }
        }

        return Ok(false);
    }

    fn div_optimization(
        &self,
        function: FunctionValue<'c>,
        value_table: PointerValue<'c>,
        counter: PointerValue<'c>,
        v: &[BfAST],
        loop_end: &BasicBlock,
    ) -> Result<bool> {
        // TODO: too dirty; needs to refactor

        if let [BfAST::SubOp(i), BfAST::AddPtr(j), BfAST::AddOp(1), BfAST::SubPtr(k)] = v[0..4] {
            if j == k {
                let cur = self.get_current(value_table, counter);
                let rat = self.context.i8_type().const_int(i as u64, false);

                let modulo = self.builder.build_int_unsigned_rem(cur, rat, "");
                
                let br_okay = self.context.append_basic_block(function, "");
                let br_not_okay = self.context.append_basic_block(function, "");

                self.builder.build_conditional_branch(
                    self.builder.build_int_compare(IntPredicate::EQ, modulo, self.context.i8_type().const_int(0 as u64, false), ""),
                     &br_okay, &br_not_okay);
                
                self.builder.position_at_end(&br_okay);

                let dest_pos = self.builder.build_int_add(
                    self.builder.build_load(counter, "").into_int_value(),
                    self.context.i64_type().const_int(j as u64, false),
                    "",
                );

                let dest_ref = unsafe {
                    self.builder.build_in_bounds_gep(
                        value_table,
                        &[self.context.i64_type().const_int(0, false), dest_pos],
                        "",
                    )
                };

                let dest = self.builder.build_load(dest_ref, "").into_int_value();
                let res = self.builder
                              .build_int_add(dest, self.builder.build_int_unsigned_div(cur, rat, ""), "");

                self.builder.build_store(dest_ref, res);

                self.set_current(
                    value_table,
                    counter,
                    self.context.i8_type().const_int(0, false),
                );

                self.builder.build_unconditional_branch(loop_end);
                
                self.builder.position_at_end(&br_not_okay);

                return Ok(true);
            }
        } else if let [BfAST::SubOp(i), BfAST::SubPtr(j), BfAST::AddOp(1), BfAST::AddPtr(k)] = v[0..4] {
            if j == k {
                let cur = self.get_current(value_table, counter);
                let rat = self.context.i8_type().const_int(i as u64, false);

                let modulo = self.builder.build_int_unsigned_rem(cur, rat, "");
                
                let br_okay = self.context.append_basic_block(function, "");
                let br_not_okay = self.context.append_basic_block(function, "");

                self.builder.build_conditional_branch(
                    self.builder.build_int_compare(IntPredicate::EQ, modulo, self.context.i8_type().const_int(0 as u64, false), ""),
                     &br_okay, &br_not_okay);
                
                self.builder.position_at_end(&br_okay);

                let dest_pos = self.builder.build_int_sub(
                    self.builder.build_load(counter, "").into_int_value(),
                    self.context.i64_type().const_int(j as u64, false),
                    "",
                );

                let dest_ref = unsafe {
                    self.builder.build_in_bounds_gep(
                        value_table,
                        &[self.context.i64_type().const_int(0, false), dest_pos],
                        "",
                    )
                };

                let dest = self.builder.build_load(dest_ref, "").into_int_value();
                let res = self.builder
                              .build_int_add(dest, self.builder.build_int_unsigned_div(cur, rat, ""), "");

                self.builder.build_store(dest_ref, res);

                self.set_current(
                    value_table,
                    counter,
                    self.context.i8_type().const_int(0, false),
                );

                self.builder.build_unconditional_branch(loop_end);
                
                self.builder.position_at_end(&br_not_okay);

                return Ok(true);
            }
        } else if let [BfAST::SubOp(i), BfAST::AddPtr(j), BfAST::SubOp(1), BfAST::SubPtr(k)] = v[0..4] {
            if j == k {
                let cur = self.get_current(value_table, counter);
                let rat = self.context.i8_type().const_int(i as u64, false);

                let modulo = self.builder.build_int_unsigned_rem(cur, rat, "");
                
                let br_okay = self.context.append_basic_block(function, "");
                let br_not_okay = self.context.append_basic_block(function, "");

                self.builder.build_conditional_branch(
                    self.builder.build_int_compare(IntPredicate::EQ, modulo, self.context.i8_type().const_int(0 as u64, false), ""),
                     &br_okay, &br_not_okay);
                
                self.builder.position_at_end(&br_okay);

                let dest_pos = self.builder.build_int_add(
                    self.builder.build_load(counter, "").into_int_value(),
                    self.context.i64_type().const_int(j as u64, false),
                    "",
                );

                let dest_ref = unsafe {
                    self.builder.build_in_bounds_gep(
                        value_table,
                        &[self.context.i64_type().const_int(0, false), dest_pos],
                        "",
                    )
                };

                let dest = self.builder.build_load(dest_ref, "").into_int_value();
                let res = self.builder
                              .build_int_sub(dest, self.builder.build_int_unsigned_div(cur, rat, ""), "");

                self.builder.build_store(dest_ref, res);

                self.set_current(
                    value_table,
                    counter,
                    self.context.i8_type().const_int(0, false),
                );

                self.builder.build_unconditional_branch(loop_end);
                
                self.builder.position_at_end(&br_not_okay);

                return Ok(true);
            }
        } else if let [BfAST::SubOp(i), BfAST::SubPtr(j), BfAST::SubOp(1), BfAST::AddPtr(k)] = v[0..4] {
            if j == k {
                let cur = self.get_current(value_table, counter);
                let rat = self.context.i8_type().const_int(i as u64, false);

                let modulo = self.builder.build_int_unsigned_rem(cur, rat, "");
                
                let br_okay = self.context.append_basic_block(function, "");
                let br_not_okay = self.context.append_basic_block(function, "");

                self.builder.build_conditional_branch(
                    self.builder.build_int_compare(IntPredicate::EQ, modulo, self.context.i8_type().const_int(0 as u64, false), ""),
                     &br_okay, &br_not_okay);
                
                self.builder.position_at_end(&br_okay);

                let dest_pos = self.builder.build_int_sub(
                    self.builder.build_load(counter, "").into_int_value(),
                    self.context.i64_type().const_int(j as u64, false),
                    "",
                );

                let dest_ref = unsafe {
                    self.builder.build_in_bounds_gep(
                        value_table,
                        &[self.context.i64_type().const_int(0, false), dest_pos],
                        "",
                    )
                };

                let dest = self.builder.build_load(dest_ref, "").into_int_value();
                let res = self.builder
                              .build_int_sub(dest, self.builder.build_int_unsigned_div(cur, rat, ""), "");

                self.builder.build_store(dest_ref, res);

                self.set_current(
                    value_table,
                    counter,
                    self.context.i8_type().const_int(0, false),
                );

                self.builder.build_unconditional_branch(loop_end);
                
                self.builder.position_at_end(&br_not_okay);

                return Ok(true);
            }
        }
        return Ok(false);
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
                if v.is_empty() {
                    return Ok(());
                } else if v.len() == 1 {
                    if let BfAST::SubOp(_) = v[0] {
                        self.set_current(
                            value_table,
                            counter,
                            self.context.i8_type().const_int(0 as u64, false),
                        );

                        return Ok(());
                    }
                } else if v.len() == 4 {
                    // balanced loop optimization (frequently used on multiplications)
                    if self.balanced_loop_optimization(value_table, counter, &v)? {
                        return Ok(());
                    }
                }

                let loop_head = self.context.append_basic_block(function, "");
                let loop_body = self.context.append_basic_block(function, "");
                let loop_end = self.context.append_basic_block(function, "");

                if v.len() == 4 {
                    // division optimization
                    self.div_optimization(function, value_table, counter, &v, &loop_end)?;
                }

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
