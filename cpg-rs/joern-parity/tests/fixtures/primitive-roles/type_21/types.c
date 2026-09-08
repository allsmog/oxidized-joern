unsigned long int identity(unsigned long int value) { unsigned long int local = value; return local; }
unsigned long int declared(unsigned long int value);
unsigned long int call_definition(unsigned long int value) { return identity(value); }
unsigned long int call_prototype(unsigned long int value) { return declared(value); }
