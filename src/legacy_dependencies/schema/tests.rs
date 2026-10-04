use crate::completion_schema::tests::assert_fields;
use crate::strict_json::shape::Shape;

#[test]
fn dependency_decoder_fields_require_concrete_raw_guards() {
    let Shape::Record(fields) = &super::REQUEST else {
        panic!("request record");
    };
    assert_fields::<super::super::Input>(&Shape::Fields(fields), &[]);
    let Shape::Record(fields) = &super::INTERVAL else {
        panic!("interval record");
    };
    assert_fields::<super::super::Interval>(&Shape::Fields(fields), &[]);
    let Shape::Record(fields) = &super::ACK else {
        panic!("acknowledgement record");
    };
    assert_fields::<super::super::Acknowledgement>(&Shape::Fields(fields), &[]);
}
