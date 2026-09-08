#if 0
int inactive_block_prototype(int x) { extern int inactive_external(int); return inactive_external(x); }
int inactive_top_prototype(int x);
#else
int active_block_prototype(int x) { extern int active_external(int); return active_external(x); }
int active_top_prototype(int x);
#endif
