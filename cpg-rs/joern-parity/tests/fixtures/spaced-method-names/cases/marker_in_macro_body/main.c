#define VALUE(x) ((x)+1) /* FULL_NAME=other */
int other(int x){return x;}
int invoke(int x){return VALUE(x);}
