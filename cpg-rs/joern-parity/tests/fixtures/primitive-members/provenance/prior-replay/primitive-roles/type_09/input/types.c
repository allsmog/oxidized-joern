int identity(int value) { int local = value; return local; }
int declared(int value);
int call_definition(int value) { return identity(value); }
int call_prototype(int value) { return declared(value); }
