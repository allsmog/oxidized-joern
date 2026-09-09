short int identity(short int value) { short int local = value; return local; }
short int declared(short int value);
short int call_definition(short int value) { return identity(value); }
short int call_prototype(short int value) { return declared(value); }
