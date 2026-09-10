unsigned int identity(unsigned int value) { unsigned int local = value; return local; }
unsigned int declared(unsigned int value);
unsigned int call_definition(unsigned int value) { return identity(value); }
unsigned int call_prototype(unsigned int value) { return declared(value); }
