#define PREFIX(op,x) (op (x))
#define PICK(a,b,c) (b)
int empty(int value) { return PREFIX(,value) + PICK(,value,); }
