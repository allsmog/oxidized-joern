unsigned long identity(unsigned long value) { unsigned long local = value; return local; }
unsigned long declared(unsigned long value);
unsigned long call_definition(unsigned long value) { return identity(value); }
unsigned long call_prototype(unsigned long value) { return declared(value); }
