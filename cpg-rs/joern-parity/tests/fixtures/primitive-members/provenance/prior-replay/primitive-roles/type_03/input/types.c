short identity(short value) { short local = value; return local; }
short declared(short value);
short call_definition(short value) { return identity(value); }
short call_prototype(short value) { return declared(value); }
