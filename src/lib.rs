extern crate bech32;
extern crate bitcoin_hashes;
extern crate num_traits;
extern crate regex;
extern crate secp256k1;

use bech32::u5;
use bitcoin_hashes::Hash;
use bitcoin_hashes::sha256::Sha256Hash;

use secp256k1::key::PublicKey;
use secp256k1::{Message, RecoverableSignature, Secp256k1};
use std::ops::Deref;

use std::iter::FilterMap;
use std::slice::Iter;

mod de;
mod ser;

/// Represents a semantically correct lightning BOLT11 invoice
pub struct Invoice {
	signed_invoice: SignedRawInvoice,

}

pub enum InvoiceDescription<'f> {
	Direct(&'f Description),
	Hash(&'f Sha256)
}

/// Represents a signed `RawInvoice` with cached hash.
///
/// # Invariants
/// * The hash has to be either from the deserialized invoice or from the serialized `raw_invoice`
#[derive(Eq, PartialEq, Debug)]
pub struct SignedRawInvoice {
	/// The rawInvoice that the signature belongs to
	raw_invoice: RawInvoice,

	/// Hash of the `RawInvoice` that will be used to check the signature.
	///
	/// * if the `SignedRawInvoice` was deserialized the hash is of from the original encoded form,
	/// since it's not guaranteed that encoding it again will lead to the same result since integers
	/// could have been encoded with leading zeroes etc.
	/// * if the `SignedRawInvoice` was constructed manually the hash will be the calculated hash
	/// from the `RawInvoice`
	hash: [u8; 32],

	/// signature of the payment request
	signature: Signature,
}

/// Represents an syntactically correct Invoice for a payment on the lightning network as defined in
/// [BOLT #11](https://github.com/lightningnetwork/lightning-rfc/blob/master/11-payment-encoding.md),
/// but without the signature information.
/// De- and encoding should not lead to information loss.
#[derive(Eq, PartialEq, Debug)]
pub struct RawInvoice {
	/// human readable part
	pub hrp: RawHrp,

	/// data part
	pub data: RawDataPart,
}

/// Data of the `RawInvoice` that is encoded in the human readable part
#[derive(Eq, PartialEq, Debug)]
pub struct RawHrp {
	/// The currency deferred from the 3rd and 4th character of the bech32 transaction
	pub currency: Currency,

	/// The amount that, multiplied by the SI prefix, has to be payed
	pub raw_amount: Option<u64>,

	/// SI prefix that gets multiplied with the `raw_amount`
	pub si_prefix: Option<SiPrefix>,
}

/// Data of the `RawInvoice` that is encoded in the data part
#[derive(Eq, PartialEq, Debug)]
pub struct RawDataPart {
	// TODO: find better fitting type that only allows positive timestamps to avoid checks for negative timestamps when encoding
	/// generation time of the invoice as UNIX timestamp
	pub timestamp: u64,

	/// tagged fields of the payment request
	pub tagged_fields: Vec<RawTaggedField>,
}

/// SI prefixes for the human readable part
#[derive(Eq, PartialEq, Debug)]
pub enum SiPrefix {
	/// 10^-3
	Milli,
	/// 10^-6
	Micro,
	/// 10^-9
	Nano,
	/// 10^-12
	Pico,
}

#[derive(Eq, PartialEq, Debug)]
pub enum Currency {
	Bitcoin,
	BitcoinTestnet,
}

/// Tagged field which may have an unknown tag
#[derive(Eq, PartialEq, Debug)]
pub enum RawTaggedField {
	/// Parsed tagged field with known tag
	KnownSemantics(TaggedField),
	/// tagged field which was not parsed due to an unknown tag or undefined field semantics
	UnknownSemantics(Vec<u5>),
}

/// Tagged field with known tag
#[derive(Eq, PartialEq, Debug)]
pub enum TaggedField {
	PaymentHash(Sha256),
	Description(Description),
	PayeePubKey(PayeePubKey),
	DescriptionHash(Sha256),
	ExpiryTime(ExpiryTime),
	MinFinalCltvExpiry(MinFinalCltvExpiry),
	Fallback(Fallback),
	Route(Route),
}

// TODO: use struct from bitcoin_hashes
/// SHA-256 hash
#[derive(Eq, PartialEq, Debug)]
pub struct Sha256(pub [u8; 32]);

/// Description string
///
/// # Invariants
/// The description can be at most 639 __bytes__ long
#[derive(Eq, PartialEq, Debug)]
pub struct Description(String);

/// Payee public key
#[derive(Eq, PartialEq, Debug)]
pub struct PayeePubKey(pub PublicKey);

/// Positive duration that defines when (relatively to the timestamp) in the future the invoice expires
#[derive(Eq, PartialEq, Debug)]
pub struct ExpiryTime {
	pub seconds: u64
}

/// `min_final_cltv_expiry` to use for the last HTLC in the route
#[derive(Eq, PartialEq, Debug)]
pub struct MinFinalCltvExpiry(pub u64);

// TODO: better types instead onf byte arrays
/// Fallback address in case no LN payment is possible
#[derive(Eq, PartialEq, Debug)]
pub enum Fallback {
	SegWitProgram {
		version: u5,
		program: Vec<u8>,
	},
	PubKeyHash([u8; 20]),
	ScriptHash([u8; 20]),
}

/// Recoverable signature
#[derive(Eq, PartialEq, Debug)]
pub struct Signature(pub RecoverableSignature);

/// Private routing information
///
/// # Invariants
/// The encoded route has to be <1024 5bit characters long (<=639 bytes or <=12 hops)
///
#[derive(Eq, PartialEq, Debug)]
pub struct Route(Vec<RouteHop>);

#[derive(Eq, PartialEq, Debug)]
pub struct RouteHop {
	pub pubkey: PublicKey,
	pub short_channel_id: [u8; 8],
	pub fee_base_msat: u32,
	pub fee_proportional_millionths: u32,
	pub cltv_expiry_delta: u16,
}

pub mod constants {
	use bech32::u5;

	pub const TAG_PAYMENT_HASH: u8 = 1;
	pub const TAG_DESCRIPTION: u8 = 13;
	pub const TAG_PAYEE_PUB_KEY: u8 = 19;
	pub const TAG_DESCRIPTION_HASH: u8 = 23;
	pub const TAG_EXPIRY_TIME: u8 = 6;
	pub const TAG_MIN_FINAL_CLTV_EXPIRY: u8 = 24;
	pub const TAG_FALLBACK: u8 = 9;
	pub const TAG_ROUTE: u8 = 3;
}

/// # FOR INTERNAL USE ONLY! READ BELOW!
///
/// It's a convenience function to convert `u8` tags to `u5` tags. Therefore `tag` has to
/// be in range `[0..32]`.
///
/// # Panics
/// If the `tag` value is not in the range `[0..32]`.
fn as_u5(tag: u8) -> u5 {
	u5::try_from_u8(tag).unwrap()
}

impl SignedRawInvoice {
	/// Disassembles the `SignedRawInvoice` into it's three parts:
	///  1. raw invoice
	///  2. hash of the raw invoice
	///  3. signature
	pub fn into_parts(self) -> (RawInvoice, [u8; 32], Signature) {
		(self.raw_invoice, self.hash, self.signature)
	}

	pub fn raw_invoice(&self) -> &RawInvoice {
		&self.raw_invoice
	}

	pub fn hash(&self) -> &[u8; 32] {
		&self.hash
	}

	pub fn signature(&self) -> &Signature {
		&self.signature
	}

	pub fn recover_payee_pub_key(&self) -> Result<PayeePubKey, secp256k1::Error> {
		let hash = Message::from_slice(&self.hash[..])
			.expect("Hash is 32 bytes long, same as MESSAGE_SIZE");

		Ok(PayeePubKey(Secp256k1::new().recover(
			&hash,
			&self.signature
		)?))
	}

	pub fn check_signature(&self) -> bool {
		let included_pub_key = self.raw_invoice.payee_pub_key();

		let mut recovered_pub_key = Option::None;
		if recovered_pub_key.is_none() {
			let recovered = match self.recover_payee_pub_key() {
				Ok(pk) => pk,
				Err(_) => return false,
			};
			recovered_pub_key = Some(recovered);
		}

		let pub_key = included_pub_key.or(recovered_pub_key.as_ref())
			.expect("One is always present");

		let hash = Message::from_slice(&self.hash[..])
			.expect("Hash is 32 bytes long, same as MESSAGE_SIZE");

		let secp_context = Secp256k1::new();
		let verification_result = secp_context.verify(
			&hash,
			&self.signature.to_standard(&secp_context),
			pub_key
		);

		match verification_result {
			Ok(()) => true,
			Err(_) => false,
		}
	}
}

/// Finds the first element of an enum stream of a given variant and extracts one member of the
/// variant. If no element was found `None` gets returned.
///
/// The following example would extract the first
/// ```
/// use Enum::*
///
/// enum Enum {
/// 	A(u8),
/// 	B(u16)
/// }
///
/// let elements = vec![A(1), A(2), B(3), A(4)]
///
/// assert_eq!(find_extract!(elements.iter(), Enum::B(ref x), x), Some(3u16))
/// ```
macro_rules! find_extract {
    ($iter:expr, $enm:pat, $enm_var:ident) => {
    	$iter.filter_map(|tf| match tf {
			&$enm => Some($enm_var),
			_ => None,
		}).next()
    };
}

impl RawInvoice {
	fn hash_from_parts(hrp_bytes: &[u8], data_without_signature: &[u5]) -> [u8; 32] {
		use bech32::FromBase32;

		let mut preimage = Vec::<u8>::from(hrp_bytes);

		let mut data_part = Vec::from(data_without_signature);
		let overhang = (data_part.len() * 5) % 8;
		if overhang > 0 {
			// add padding if data does not end at a byte boundary
			data_part.push(u5::try_from_u8(0).unwrap());

			// if overhang is in (1..3) we need to add u5(0) padding two times
			if overhang < 3 {
				data_part.push(u5::try_from_u8(0).unwrap());
			}
		}

		preimage.extend_from_slice(&Vec::<u8>::from_base32(&data_part)
			.expect("No padding error may occur due to appended zero above."));

		let mut hash: [u8; 32] = Default::default();
		hash.copy_from_slice(&Sha256Hash::hash(&preimage)[..]);
		hash
	}

	pub fn hash(&self) -> [u8; 32] {
		use bech32::ToBase32;

		RawInvoice::hash_from_parts(
			self.hrp.to_string().as_bytes(),
			&self.data.to_base32()
		)
	}

	pub fn known_tagged_fields(&self)
		-> FilterMap<Iter<RawTaggedField>, fn(&RawTaggedField) -> Option<&TaggedField>>
	{
		// For 1.14.0 compatibility: closures' types can't be written an fn()->() in the
		// function's type signature.
		// TODO: refactor once impl Trait is available
		fn match_raw(raw: &RawTaggedField) -> Option<&TaggedField> {
			match raw {
				&RawTaggedField::KnownSemantics(ref tf) => Some(tf),
				_ => None,
			}
		}

		self.data.tagged_fields.iter().filter_map(match_raw )
	}

	pub fn payment_hash(&self) -> Option<&Sha256> {
		find_extract!(self.known_tagged_fields(), TaggedField::PaymentHash(ref x), x)
	}

	pub fn description(&self) -> Option<&Description> {
		find_extract!(self.known_tagged_fields(), TaggedField::Description(ref x), x)
	}

	pub fn payee_pub_key(&self) -> Option<&PayeePubKey> {
		find_extract!(self.known_tagged_fields(), TaggedField::PayeePubKey(ref x), x)
	}

	pub fn description_hash(&self) -> Option<&Sha256> {
		find_extract!(self.known_tagged_fields(), TaggedField::DescriptionHash(ref x), x)
	}

	pub fn expiry_time(&self) -> Option<&ExpiryTime> {
		find_extract!(self.known_tagged_fields(), TaggedField::ExpiryTime(ref x), x)
	}

	pub fn min_final_cltv_expiry(&self) -> Option<&MinFinalCltvExpiry> {
		find_extract!(self.known_tagged_fields(), TaggedField::MinFinalCltvExpiry(ref x), x)
	}

	pub fn fallbacks(&self) -> Vec<&Fallback> {
		self.known_tagged_fields().filter_map(|tf| match tf {
			&TaggedField::Fallback(ref f) => Some(f),
			num_traits => None,
		}).collect::<Vec<&Fallback>>()
	}

	pub fn routes(&self) -> Vec<&Route> {
		self.known_tagged_fields().filter_map(|tf| match tf {
			&TaggedField::Route(ref r) => Some(r),
			num_traits => None,
		}).collect::<Vec<&Route>>()
	}
}

impl Invoice {
	/// Check that all mandatory fields are present
	fn check_field_counts(&self) -> Result<(), SemanticError> {
		// "A writer MUST include exactly one p field […]."
		let payment_hash_cnt = self.tagged_fields().filter(|tf| match tf {
			TaggedField::PaymentHash(_) => true,
			_ => false,
		}).count();
		if payment_hash_cnt < 1 {
			return Err(SemanticError::NoPaymentHash);
		} else if payment_hash_cnt > 1 {
			return Err(SemanticError::MultiplePaymentHashes);
		}

		// "A writer MUST include either exactly one d or exactly one h field."
		let description_cnt = self.tagged_fields().filter(|tf| match tf {
			TaggedField::Description(_) | TaggedField::DescriptionHash(_) => true,
			_ => false,
		}).count();
		if  description_cnt < 1 {
			return Err(SemanticError::NoDescription);
		} else if description_cnt > 1 {
			return  Err(SemanticError::MultipleDescriptions);
		}

		Ok(())
	}

	/// Check that the invoice is signed correctly and that key recovery works
	fn check_signature(&self) -> Result<(), SemanticError> {
		match self.signed_invoice.recover_payee_pub_key() {
			Err(secp256k1::Error::InvalidRecoveryId) =>
				return Err(SemanticError::InvalidRecoveryId),
			Err(_) => panic!("no other error may occur"),
			Ok(_) => {},
		}

		if !self.signed_invoice.check_signature() {
			return Err(SemanticError::InvalidSignature);
		}

		Ok(())
	}

	pub fn from_signed(signed_invoice: SignedRawInvoice) -> Result<Self, SemanticError> {
		let invoice = Invoice {
			signed_invoice: signed_invoice,
		};
		invoice.check_field_counts()?;
		invoice.check_signature()?;

		Ok(invoice)
	}

	pub fn tagged_fields(&self)
		-> FilterMap<Iter<RawTaggedField>, fn(&RawTaggedField) -> Option<&TaggedField>> {
		self.signed_invoice.raw_invoice().known_tagged_fields()
	}

	pub fn payment_hash(&self) -> &Sha256 {
		self.signed_invoice.payment_hash().expect("checked by constructor")
	}

	pub fn description(&self) -> InvoiceDescription {
		if let Some(ref direct) = self.signed_invoice.description() {
			return InvoiceDescription::Direct(direct);
		} else if let Some(ref hash) = self.signed_invoice.description_hash() {
			return InvoiceDescription::Hash(hash);
		}
		unreachable!("ensured by constructor");
	}

	pub fn payee_pub_key(&self) -> Option<&PayeePubKey> {
		self.signed_invoice.payee_pub_key()
	}

	pub fn recover_payee_pub_key(&self) -> PayeePubKey {
		self.signed_invoice.recover_payee_pub_key().expect("was checked by constructor")
	}

	pub fn expiry_time(&self) -> Option<&ExpiryTime> {
		self.signed_invoice.expiry_time()
	}

	pub fn min_final_cltv_expiry(&self) -> Option<&MinFinalCltvExpiry> {
		self.signed_invoice.min_final_cltv_expiry()
	}

	pub fn fallbacks(&self) -> Vec<&Fallback> {
		self.signed_invoice.fallbacks()
	}

	pub fn routes(&self) -> Vec<&Route> {
		self.signed_invoice.routes()
	}
}

impl From<TaggedField> for RawTaggedField {
	fn from(tf: TaggedField) -> Self {
		RawTaggedField::KnownSemantics(tf)
	}
}

impl TaggedField {
	pub fn tag(&self) -> u5 {
		let tag = match *self {
			TaggedField::PaymentHash(_) => constants::TAG_PAYMENT_HASH,
			TaggedField::Description(_) => constants::TAG_DESCRIPTION,
			TaggedField::PayeePubKey(_) => constants::TAG_PAYEE_PUB_KEY,
			TaggedField::DescriptionHash(_) => constants::TAG_DESCRIPTION_HASH,
			TaggedField::ExpiryTime(_) => constants::TAG_EXPIRY_TIME,
			TaggedField::MinFinalCltvExpiry(_) => constants::TAG_MIN_FINAL_CLTV_EXPIRY,
			TaggedField::Fallback(_) => constants::TAG_FALLBACK,
			TaggedField::Route(_) => constants::TAG_ROUTE,
		};

		u5::try_from_u8(tag).expect("all tags defined are <32")
	}
}

impl Description {

	/// Creates a new `Description` if `description` is at most 1023 __bytes__ long,
	/// returns `CreationError::DescriptionTooLong` otherwise
	///
	/// Please note that single characters may use more than one byte due to UTF8 encoding.
	pub fn new(description: String) -> Result<Description, CreationError> {
		if description.len() > 639 {
			Err(CreationError::DescriptionTooLong)
		} else {
			Ok(Description(description))
		}
	}

	pub fn into_inner(self) -> String {
		self.0
	}
}

impl Into<String> for Description {
	fn into(self) -> String {
		self.into_inner()
	}
}

impl Deref for Description {
	type Target = str;

	fn deref(&self) -> &str {
		&self.0
	}
}

impl From<PublicKey> for PayeePubKey {
	fn from(pk: PublicKey) -> Self {
		PayeePubKey(pk)
	}
}

impl Deref for PayeePubKey {
	type Target = PublicKey;

	fn deref(&self) -> &PublicKey {
		&self.0
	}
}

impl Route {
	pub fn new(hops: Vec<RouteHop>) -> Result<Route, CreationError> {
		if hops.len() <= 12 {
			Ok(Route(hops))
		} else {
			Err(CreationError::RouteTooLong)
		}
	}

	fn into_inner(self) -> Vec<RouteHop> {
		self.0
	}
}

impl Into<Vec<RouteHop>> for Route {
	fn into(self) -> Vec<RouteHop> {
		self.into_inner()
	}
}

impl Deref for Route {
	type Target = Vec<RouteHop>;

	fn deref(&self) -> &Vec<RouteHop> {
		&self.0
	}
}

impl Deref for Signature {
	type Target = RecoverableSignature;

	fn deref(&self) -> &RecoverableSignature {
		&self.0
	}
}

impl Deref for SignedRawInvoice {
	type Target = RawInvoice;

	fn deref(&self) -> &RawInvoice {
		&self.raw_invoice
	}
}

/// Errors that may occur when constructing a new `RawInvoice` or `Invoice`
#[derive(Eq, PartialEq, Debug)]
pub enum CreationError {
	/// The supplied description string was longer than 639 __bytes__ (see [`Description::new(…)`](./struct.Description.html#method.new))
	DescriptionTooLong,

	/// The specified route has too many hops and can't be encoded
	RouteTooLong,
}

/// Errors that may occur when converting a `RawInvoice` to an `Invoice`. They relate to the
/// requirements sections in BOLT #11
#[derive(Eq, PartialEq, Debug)]
pub enum SemanticError {
	NoPaymentHash,
	MultiplePaymentHashes,

	NoDescription,
	MultipleDescriptions,

	InvalidRecoveryId,
	InvalidSignature,
}

#[cfg(test)]
mod test {

	#[test]
	fn test_calc_invoice_hash() {
		use ::{RawInvoice, RawHrp, RawDataPart, Currency};
		use secp256k1::*;
		use ::TaggedField::*;

		let invoice = RawInvoice {
			hrp: RawHrp {
				currency: Currency::Bitcoin,
				raw_amount: None,
				si_prefix: None,
			},
			data: RawDataPart {
				timestamp: 1496314658,
				tagged_fields: vec![
					PaymentHash(::Sha256([
						0x00u8, 0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08, 0x09, 0x00,
						0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08, 0x09, 0x00, 0x01,
						0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08, 0x09, 0x01, 0x02
					])).into(),
					Description(::Description::new("Please consider supporting this project".to_owned()).unwrap()).into(),
				],
			},
		};

		let expected_hash = [
			0xc3, 0xd4, 0xe8, 0x3f, 0x64, 0x6f, 0xa7, 0x9a, 0x39, 0x3d, 0x75, 0x27, 0x7b, 0x1d,
			0x85, 0x8d, 0xb1, 0xd1, 0xf7, 0xab, 0x71, 0x37, 0xdc, 0xb7, 0x83, 0x5d, 0xb2, 0xec,
			0xd5, 0x18, 0xe1, 0xc9
		];

		assert_eq!(invoice.hash(), expected_hash)
	}

	#[test]
	fn test_check_signature() {
		use TaggedField::*;
		use secp256k1::{RecoveryId, RecoverableSignature, Secp256k1};
		use secp256k1::key::{SecretKey, PublicKey};
		use {SignedRawInvoice, Signature, RawInvoice, RawHrp, RawDataPart, Currency, Sha256};

		let mut invoice = SignedRawInvoice {
			raw_invoice: RawInvoice {
				hrp: RawHrp {
					currency: Currency::Bitcoin,
					raw_amount: None,
					si_prefix: None,
				},
				data: RawDataPart {
					timestamp: 1496314658,
					tagged_fields: vec ! [
						PaymentHash(Sha256([
							0x00u8, 0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08, 0x09, 0x00,
							0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08, 0x09, 0x00, 0x01,
							0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08, 0x09, 0x01, 0x02
						])).into(),
						Description(
							::Description::new(
								"Please consider supporting this project".to_owned()
							).unwrap()
						).into(),
					],
				},
			},
			hash: [
				0xc3, 0xd4, 0xe8, 0x3f, 0x64, 0x6f, 0xa7, 0x9a, 0x39, 0x3d, 0x75, 0x27,
				0x7b, 0x1d, 0x85, 0x8d, 0xb1, 0xd1, 0xf7, 0xab, 0x71, 0x37, 0xdc, 0xb7,
				0x83, 0x5d, 0xb2, 0xec, 0xd5, 0x18, 0xe1, 0xc9
			],
			signature: Signature(RecoverableSignature::from_compact(
				& Secp256k1::without_caps(),
				& [
					0x38u8, 0xec, 0x68, 0x91, 0x34, 0x5e, 0x20, 0x41, 0x45, 0xbe, 0x8a,
					0x3a, 0x99, 0xde, 0x38, 0xe9, 0x8a, 0x39, 0xd6, 0xa5, 0x69, 0x43,
					0x4e, 0x18, 0x45, 0xc8, 0xaf, 0x72, 0x05, 0xaf, 0xcf, 0xcc, 0x7f,
					0x42, 0x5f, 0xcd, 0x14, 0x63, 0xe9, 0x3c, 0x32, 0x88, 0x1e, 0xad,
					0x0d, 0x6e, 0x35, 0x6d, 0x46, 0x7e, 0xc8, 0xc0, 0x25, 0x53, 0xf9,
					0xaa, 0xb1, 0x5e, 0x57, 0x38, 0xb1, 0x1f, 0x12, 0x7f
				],
				RecoveryId::from_i32(0).unwrap()
			).unwrap()),
		};

		assert!(invoice.check_signature());

		let private_key = SecretKey::from_slice(
			&Secp256k1::without_caps(),
			&[
				0xe1, 0x26, 0xf6, 0x8f, 0x7e, 0xaf, 0xcc, 0x8b, 0x74, 0xf5, 0x4d, 0x26, 0x9f, 0xe2,
				0x06, 0xbe, 0x71, 0x50, 0x00, 0xf9, 0x4d, 0xac, 0x06, 0x7d, 0x1c, 0x04, 0xa8, 0xca,
				0x3b, 0x2d, 0xb7, 0x34
			][..]
		).unwrap();
		let public_key = PublicKey::from_secret_key(&Secp256k1::new(), &private_key);

		assert_eq!(invoice.recover_payee_pub_key(), Ok(::PayeePubKey(public_key)));
	}
}