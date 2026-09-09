#define BRACE(v) { int temp=(v); sink(temp); }
#define LOOP(v) do { int temp=(v); sink(temp); } while(0)
#define DROP(v) do { sink(0); } while(0)
#define SAFE(v) { int temp=(v); temp=0; sink(temp); }
#define IF(v,c) if(c) { sink(v); }
#define REPEAT(v,c) while(c) { sink(v); break; }
int source(void);
void sink(int value);
void brace_entry(void) { int value=source(); BRACE(value); }
void do_entry(void) { int value=source(); LOOP(value); }
void dropped_entry(void) { int value=source(); DROP(value); }
void overwritten_entry(void) { int value=source(); SAFE(value); }
void if_entry(int condition) { int value=source(); IF(value,condition); }
void while_entry(int condition) { int value=source(); REPEAT(value,condition); }
void clean_entry(void) { BRACE(0); }
