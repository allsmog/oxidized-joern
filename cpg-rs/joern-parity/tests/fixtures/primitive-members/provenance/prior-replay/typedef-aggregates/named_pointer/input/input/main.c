typedef struct Tag { int value; char bytes[2]; } *Ptr, Value;
int read(Ptr p) { return p->value; }
