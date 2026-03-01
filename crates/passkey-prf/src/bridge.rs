//! Core bridge to Apple's AuthenticationServices framework for passkey PRF operations.
// Delegate method names must match Objective-C selectors, not Rust conventions.
#![allow(non_snake_case)]

use std::fmt;
use std::sync::{Arc, Mutex};

use objc2::rc::Retained;
use objc2::runtime::{AnyObject, NSObject, ProtocolObject};
use objc2::{
    define_class, msg_send, AnyThread, DefinedClass, MainThreadMarker, MainThreadOnly, Message,
};
use objc2_app_kit::NSWindow;
use objc2_authentication_services::*;
use objc2_core_foundation::{kCFRunLoopDefaultMode, CFRunLoop};
use objc2_foundation::*;

/// Result of a successful passkey registration.
#[derive(Debug, Clone)]
pub struct Registration {
    /// The credential ID assigned by the authenticator.
    pub credential_id: Vec<u8>,
    /// Whether PRF is supported by this credential.
    pub prf_supported: bool,
}

/// Errors from passkey PRF operations.
#[derive(Debug)]
pub enum PrfError {
    /// User cancelled the operation.
    Cancelled,
    /// PRF extension not supported by the authenticator.
    PrfNotSupported,
    /// PRF output was missing from the assertion response.
    PrfOutputMissing,
    /// Authorization failed with an Apple framework error.
    AuthorizationFailed(String),
    /// Platform does not support passkeys (macOS < 15.0).
    Unsupported,
}

impl fmt::Display for PrfError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            PrfError::Cancelled => write!(f, "User cancelled the operation"),
            PrfError::PrfNotSupported => {
                write!(f, "PRF extension not supported by the authenticator")
            }
            PrfError::PrfOutputMissing => write!(f, "PRF output missing from assertion response"),
            PrfError::AuthorizationFailed(msg) => write!(f, "Authorization failed: {}", msg),
            PrfError::Unsupported => write!(f, "Platform does not support passkeys"),
        }
    }
}

impl std::error::Error for PrfError {}

/// Internal state shared between the delegate and the caller via Arc<Mutex<..>>.
enum AuthResult {
    Pending,
    Success(Retained<ASAuthorization>),
    Failure(String),
}

/// Ivars for the Objective-C delegate class.
struct DelegateIvars {
    result: Arc<Mutex<AuthResult>>,
    window: Retained<NSWindow>,
}

// Define the Objective-C delegate class that bridges callbacks to Rust.
define_class!(
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "QpkvAuthDelegate"]
    #[ivars = DelegateIvars]
    struct AuthDelegate;

    unsafe impl NSObjectProtocol for AuthDelegate {}

    unsafe impl ASAuthorizationControllerDelegate for AuthDelegate {
        #[unsafe(method(authorizationController:didCompleteWithAuthorization:))]
        unsafe fn authorizationController_didCompleteWithAuthorization(
            &self,
            _controller: &ASAuthorizationController,
            authorization: &ASAuthorization,
        ) {
            let mut result = self.ivars().result.lock().unwrap();
            *result = AuthResult::Success(authorization.retain());
        }

        #[unsafe(method(authorizationController:didCompleteWithError:))]
        unsafe fn authorizationController_didCompleteWithError(
            &self,
            _controller: &ASAuthorizationController,
            error: &NSError,
        ) {
            let mut result = self.ivars().result.lock().unwrap();
            let description = error.localizedDescription().to_string();
            *result = AuthResult::Failure(description);
        }
    }

    unsafe impl ASAuthorizationControllerPresentationContextProviding for AuthDelegate {
        #[unsafe(method_id(presentationAnchorForAuthorizationController:))]
        unsafe fn presentationAnchorForAuthorizationController(
            &self,
            _controller: &ASAuthorizationController,
        ) -> Retained<ASPresentationAnchor> {
            // ASPresentationAnchor is NSObject on macOS; NSWindow inherits from NSObject.
            let window: &NSObject = &self.ivars().window;
            window.retain()
        }
    }
);

impl AuthDelegate {
    fn new(window: Retained<NSWindow>, result: Arc<Mutex<AuthResult>>) -> Retained<Self> {
        let mtm = MainThreadMarker::from(&*window);
        let delegate = mtm.alloc::<Self>();
        let delegate = delegate.set_ivars(DelegateIvars { result, window });
        unsafe { msg_send![super(delegate), init] }
    }
}

/// Pumps the main run loop until the delegate callback sets a result.
///
/// This avoids blocking the main thread (which would deadlock, since
/// ASAuthorizationController delivers callbacks on the main run loop).
fn wait_for_result(result_arc: &Arc<Mutex<AuthResult>>) -> AuthResult {
    loop {
        // Run the main run loop for a short interval to let callbacks fire.
        CFRunLoop::run_in_mode(
            unsafe { kCFRunLoopDefaultMode },
            0.1,  // 100ms per iteration
            true, // return after handling one source
        );

        let mut guard = result_arc.lock().unwrap();
        if !matches!(*guard, AuthResult::Pending) {
            return std::mem::replace(&mut *guard, AuthResult::Pending);
        }
    }
}

/// Register a new passkey with PRF support.
///
/// **Parameters**:
/// - `window` - The NSWindow to anchor the Touch ID prompt to.
/// - `rp_id` - The relying party identifier (domain, e.g. "example.com").
/// - `user_id` - Opaque user identifier bytes.
/// - `user_name` - Human-readable user display name.
///
/// **Returns**:
/// - `Ok(Registration)` with credential ID and PRF support flag.
/// - `Err(PrfError)` on failure.
pub fn register_passkey(
    window: &NSWindow,
    rp_id: &str,
    user_id: &[u8],
    user_name: &str,
) -> Result<Registration, PrfError> {
    unsafe {
        let rp_id_ns = NSString::from_str(rp_id);
        let provider =
            ASAuthorizationPlatformPublicKeyCredentialProvider::initWithRelyingPartyIdentifier(
                ASAuthorizationPlatformPublicKeyCredentialProvider::alloc(),
                &rp_id_ns,
            );

        // Generate a random challenge.
        let mut challenge_bytes = [0u8; 32];
        getrandom::fill(&mut challenge_bytes)
            .map_err(|e| PrfError::AuthorizationFailed(e.to_string()))?;
        let challenge = NSData::from_vec(challenge_bytes.to_vec());
        let name = NSString::from_str(user_name);
        let user_id_data = NSData::from_vec(user_id.to_vec());

        let request = provider.createCredentialRegistrationRequestWithChallenge_name_userID(
            &challenge,
            &name,
            &user_id_data,
        );

        // Enable PRF check during registration.
        let prf_input = ASAuthorizationPublicKeyCredentialPRFRegistrationInput::checkForSupport();
        request.setPrf(Some(&prf_input));

        // Perform the authorization.
        let request_as_base: Retained<ASAuthorizationRequest> = Retained::into_super(request);
        let requests = NSArray::from_retained_slice(&[request_as_base]);
        let controller = ASAuthorizationController::initWithAuthorizationRequests(
            ASAuthorizationController::alloc(),
            &requests,
        );

        let result_arc = Arc::new(Mutex::new(AuthResult::Pending));
        let delegate = AuthDelegate::new(window.retain(), result_arc.clone());
        let delegate_proto: &ProtocolObject<dyn ASAuthorizationControllerDelegate> =
            ProtocolObject::from_ref(&*delegate);
        controller.setDelegate(Some(delegate_proto));
        let presentation_proto: &ProtocolObject<
            dyn ASAuthorizationControllerPresentationContextProviding,
        > = ProtocolObject::from_ref(&*delegate);
        controller.setPresentationContextProvider(Some(presentation_proto));

        controller.performRequests();

        // Pump the run loop until the delegate callback fires.
        match wait_for_result(&result_arc) {
            AuthResult::Success(authorization) => {
                let credential = authorization.credential();
                let obj: &AnyObject = AsRef::as_ref(&*credential);
                let registration: &ASAuthorizationPlatformPublicKeyCredentialRegistration =
                    obj.downcast_ref().ok_or(PrfError::AuthorizationFailed(
                        "Unexpected credential type".to_string(),
                    ))?;

                let credential_id = {
                    let cred: &ProtocolObject<dyn ASPublicKeyCredential> =
                        ProtocolObject::from_ref(registration);
                    cred.credentialID().to_vec()
                };

                let prf_supported = registration
                    .prf()
                    .map(|output| output.isSupported())
                    .unwrap_or(false);

                Ok(Registration {
                    credential_id,
                    prf_supported,
                })
            }
            AuthResult::Failure(msg) => {
                if msg.contains("Cancel") || msg.contains("cancel") {
                    Err(PrfError::Cancelled)
                } else {
                    Err(PrfError::AuthorizationFailed(msg))
                }
            }
            AuthResult::Pending => unreachable!(),
        }
    }
}

/// Assert a passkey with PRF and retrieve the 32-byte PRF output.
///
/// **Parameters**:
/// - `window` - The NSWindow to anchor the Touch ID prompt to.
/// - `rp_id` - The relying party identifier (must match registration).
/// - `credential_id` - The credential ID from registration.
/// - `salt` - The 32-byte salt for PRF evaluation (saltInput1).
///
/// **Returns**:
/// - `Ok(Vec<u8>)` containing the 32-byte PRF output.
/// - `Err(PrfError)` on failure.
pub fn assert_prf(
    window: &NSWindow,
    rp_id: &str,
    credential_id: &[u8],
    salt: &[u8],
) -> Result<Vec<u8>, PrfError> {
    unsafe {
        let rp_id_ns = NSString::from_str(rp_id);
        let provider =
            ASAuthorizationPlatformPublicKeyCredentialProvider::initWithRelyingPartyIdentifier(
                ASAuthorizationPlatformPublicKeyCredentialProvider::alloc(),
                &rp_id_ns,
            );

        // Generate a random challenge.
        let mut challenge_bytes = [0u8; 32];
        getrandom::fill(&mut challenge_bytes)
            .map_err(|e| PrfError::AuthorizationFailed(e.to_string()))?;
        let challenge = NSData::from_vec(challenge_bytes.to_vec());

        let request = provider.createCredentialAssertionRequestWithChallenge(&challenge);

        // Set allowed credentials to target the specific passkey.
        let cred_id_data = NSData::from_vec(credential_id.to_vec());
        let descriptor = ASAuthorizationPlatformPublicKeyCredentialDescriptor::initWithCredentialID(
            ASAuthorizationPlatformPublicKeyCredentialDescriptor::alloc(),
            &cred_id_data,
        );
        let allowed = NSArray::from_retained_slice(&[descriptor]);
        request.setAllowedCredentials(&allowed);

        // Set PRF input with the salt.
        let salt_data = NSData::from_vec(salt.to_vec());
        let prf_values =
			ASAuthorizationPublicKeyCredentialPRFAssertionInputValues::initWithSaltInput1_saltInput2(
				ASAuthorizationPublicKeyCredentialPRFAssertionInputValues::alloc(),
				&salt_data,
				None,
			);
        let prf_input = ASAuthorizationPublicKeyCredentialPRFAssertionInput::initWithInputValues_perCredentialInputValues(
			ASAuthorizationPublicKeyCredentialPRFAssertionInput::alloc(),
			Some(&prf_values),
			None,
		);
        request.setPrf(Some(&prf_input));

        // Perform the authorization.
        let request_as_base: Retained<ASAuthorizationRequest> = Retained::into_super(request);
        let requests = NSArray::from_retained_slice(&[request_as_base]);
        let controller = ASAuthorizationController::initWithAuthorizationRequests(
            ASAuthorizationController::alloc(),
            &requests,
        );

        let result_arc = Arc::new(Mutex::new(AuthResult::Pending));
        let delegate = AuthDelegate::new(window.retain(), result_arc.clone());
        let delegate_proto: &ProtocolObject<dyn ASAuthorizationControllerDelegate> =
            ProtocolObject::from_ref(&*delegate);
        controller.setDelegate(Some(delegate_proto));
        let presentation_proto: &ProtocolObject<
            dyn ASAuthorizationControllerPresentationContextProviding,
        > = ProtocolObject::from_ref(&*delegate);
        controller.setPresentationContextProvider(Some(presentation_proto));

        controller.performRequests();

        // Pump the run loop until the delegate callback fires.
        match wait_for_result(&result_arc) {
            AuthResult::Success(authorization) => {
                let credential = authorization.credential();
                let obj: &AnyObject = AsRef::as_ref(&*credential);
                let assertion: &ASAuthorizationPlatformPublicKeyCredentialAssertion =
                    obj.downcast_ref().ok_or(PrfError::AuthorizationFailed(
                        "Unexpected credential type".to_string(),
                    ))?;

                let prf_output = assertion.prf().ok_or(PrfError::PrfOutputMissing)?;
                let first = prf_output.first();
                Ok(first.to_vec())
            }
            AuthResult::Failure(msg) => {
                if msg.contains("Cancel") || msg.contains("cancel") {
                    Err(PrfError::Cancelled)
                } else {
                    Err(PrfError::AuthorizationFailed(msg))
                }
            }
            AuthResult::Pending => unreachable!(),
        }
    }
}
