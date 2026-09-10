long unsigned int identity(long unsigned int value) { long unsigned int local = value; return local; }
long unsigned int declared(long unsigned int value);
long unsigned int call_definition(long unsigned int value) { return identity(value); }
long unsigned int call_prototype(long unsigned int value) { return declared(value); }
