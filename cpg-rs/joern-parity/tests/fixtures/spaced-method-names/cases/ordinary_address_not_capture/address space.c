int global=5;
int read_global(void){return global;}
void address(void){int global=7; int (*pointer)(void)=read_global;}
