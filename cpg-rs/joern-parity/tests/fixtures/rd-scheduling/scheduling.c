int source(void);
int other(void);
void sink(int value);
int identity(int value);

void rd_no_parameters() { int value = source(); while (value) { value--; } }
void rd_void_parameter(void) { int value = source(); while (value) { value--; } }
void rd_one_parameter(int count) { int value = source(); while (count) { count--; sink(value); } }
void rd_two_parameters(int count, int value) { value = source(); while (count) { count--; sink(value); } }
void rd_dead_break(void) { int value = source(); while (1) { break; sink(value); } }
void rd_dead_continue(int count) { int value = source(); while (count) { count--; continue; sink(value); } }
void rd_dead_return(void) { int value = source(); return; sink(value); }
void rd_dead_nested(int count) { int value = source(); while (count) { count--; continue; sink(identity(value)); } }
void rd_dead_assignment(int count) { int value = source(); while (count) { count--; continue; value = other(); } sink(value); }
void rd_first_loop(int count) { while (count) { count--; sink(count); } }
void rd_first_loop_two(int count, int value) { while (count) { count--; value = source(); } sink(value); }
void rd_after_loop(int count) { int value = source(); while (count) { count--; value = other(); } sink(value); }
int rd_multiple_return(int count) { int value = source(); if (count) return value; value = other(); return value; }
int rd_return_loop(int count) { int value = source(); while (count) { if (value) return value; count--; } return count; }
void rd_do_continue(int count) { int value = source(); do { count--; continue; sink(value); } while (count); }
void rd_for_continue(int count) { int value = source(); for (; count; count--) { continue; sink(value); } }
void rd_loop_break(int count) { int value = source(); while (count) { count--; if (count) break; sink(value); } }
void rd_switch_tail(int count) { int value = source(); switch (count) { case 1: break; sink(value); default: sink(value); } }
void rd_goto_tail(void) { int value = source(); goto done; sink(value); done: sink(value); }
void rd_empty(void) {}
void rd_empty_no_parameters() {}
void rd_empty_two(int first, int second) {}
void rd_branch_join(int count, int value) { if (count) value = source(); else value = other(); sink(value); }
void rd_nested_loops(int first, int second) { int value = source(); while (first) { first--; while (second) { second--; sink(value); } } }
