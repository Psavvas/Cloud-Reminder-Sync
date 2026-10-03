# Microsoft Store distribution

Microsoft signs MSIX apps distributed through the Store. Users install the
approved app without importing a developer certificate, and Windows handles updates.
The unsigned packages produced by this repository are submission inputs, not
trusted downloadable installers. Microsoft must approve and publish the app first.

## Account and app identity

1. Create and verify your developer account at https://storedeveloper.microsoft.com.
2. Reserve an available app name in Partner Center.
3. Copy the three values from **Product identity**: **Package/Identity/Name**,
   **Package/Identity/Publisher**, and **PublisherDisplayName**.

Use the values assigned to your actual app, including the exact publisher string.
Do not invent an identity or use the repository's development publisher. Store
identity differs from existing sideloaded builds; test local-data migration before
replacing an existing installation.

## Build submission packages

Run these commands from the repository root, replacing the placeholders with the
Partner Center values. These identity values are public metadata, not credentials.

```powershell
$storeIdentity = @{
    Store = $true
    IdentityName = 'PACKAGE_IDENTITY_NAME_FROM_PARTNER_CENTER'
    Publisher = 'CN=PUBLISHER_ID_FROM_PARTNER_CENTER'
    PublisherDisplayName = 'PUBLISHER_DISPLAY_NAME_FROM_PARTNER_CENTER'
}
.\scripts\build-msix.ps1 -Architecture x64 @storeIdentity
.\scripts\build-msix.ps1 -Architecture ARM64 @storeIdentity
```

Upload both `.msix` files from `dist-store` on the submission's **Packages** page.
Partner Center accepts individual MSIX packages and selects the matching
architecture for each customer's device. The build leaves the development manifest
unchanged and keeps submission files separate from sideloading outputs.

Alternatively, associate **Reminders.Package** with your app in Visual Studio and
use **Publish → Create App Packages → Microsoft Store**. The wizard can create a
`.msixupload` submission file. Use the associated Store identity for later builds.

## Submit and publish

Complete the Store listing, privacy policy, age rating, and certification notes.
Explain the `runFullTrust` capability: the desktop WinUI app runs its C# iCloud sync
engine, SQLite cache, Windows Credential Manager integration, and notifications.
Tell reviewers that demo mode is available without an Apple account, while live
sync requires an Apple account and verification. Describe the independent app
accurately; the existing Apple affiliation disclaimer applies to the listing too.

Microsoft may request changes during certification. After approval and publication,
share the Store listing link. The unsigned submission file alone does not resolve
the certificate error in Windows App Installer. Increase the app and MSIX versions
for each subsequent update and submit new packages using the same Store identity.

Official references:

- [Get started with Microsoft Store](https://learn.microsoft.com/en-us/windows/apps/publish/get-started)
- [Upload MSIX packages](https://learn.microsoft.com/en-us/windows/apps/publish/publish-your-app/msix/upload-app-packages)
- [MSIX signing guide](https://learn.microsoft.com/en-us/windows/msix/package/sign-msix-package-guide)
