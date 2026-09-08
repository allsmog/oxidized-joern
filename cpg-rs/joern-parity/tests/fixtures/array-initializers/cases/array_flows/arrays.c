int source(void);
void sink(int value);
void brace_entry(void) { int values[] = {source()}; sink(values[0]); }
void scalar_entry(void) { int value = {source()}; sink(value); }
void clean_entry(void) { int value = {0}; sink(value); }
void overwrite_entry(void) { int value = {source()}; value = 0; sink(value); }
void compound_entry(void) { sink(((int[]){source()})[0]); }
