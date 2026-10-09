using System.Net;
using System.Text.Json.Nodes;
using Reminders.Core;

internal static class AuthVerificationChecks
{
    public static async Task RunAsync(Action<bool, string> check)
    {
        foreach (var accountStatus in new[] { 200, 500 })
        {
            var verified = false;
            var diagnostics = new List<string>();
            using var auth = new AppleAuth(J.Node(new { session_token = "mock-token", hsa_version = 2 }).AsObject(),
                () => new ScriptHandler(async request =>
                {
                    var path = request.RequestUri!.AbsolutePath;
                    var body = request.Content is null ? null : J.Parse(await request.Content.ReadAsStringAsync());
                    var status = 200;
                    JsonNode response = new JsonObject();
                    if (path.EndsWith("/verify/phone"))
                    {
                        check(body.Text("mode") == "sms", "delivery explicitly requests SMS");
                        status = 409;
                    }
                    else if (path.EndsWith("/verify/phone/securitycode"))
                    {
                        check(body.Text("mode") == "sms", "verification uses the requested SMS mode despite phone metadata");
                        verified = true;
                        status = 409;
                        response = J.Node(new { securityCode = new { valid = true } });
                    }
                    else if (path.EndsWith("/accountLogin"))
                    {
                        check(verified, "account login follows accepted verification");
                        status = accountStatus;
                        response = J.Node(new { dsInfo = new { dsid = "123", hsaVersion = 2 }, hsaTrustedBrowser = true });
                    }
                    return new HttpResponseMessage((HttpStatusCode)status) { Content = new StringContent(response.ToJsonString()) };
                }));
            auth.Diagnostic += diagnostics.Add;
            // The app chooses SMS even when Apple's preferred phone mode differs.
            auth.NoteOptions(J.Node(new { trustedPhoneNumber = new { id = 7, pushMode = "voice", numberWithDialCode = "••123" } }));
            await auth.RequestCode(true, default);
            try
            {
                await auth.SubmitCode("123456", default);
                check(accountStatus == 200 && !auth.RequiresTwoFactor, "SMS finishes account sign-in");
            }
            catch (CoreException error)
            {
                check(accountStatus == 500 && error.Code == "NETWORK", "account setup failure remains a network error");
                check(error.Message.Contains("accepted the verification code") && error.Message.Contains("HTTP 500"),
                    "account setup failure distinguishes accepted code from failed account sign-in");
                check(auth.RequiresTwoFactor, "account setup failure does not authenticate the session");
            }
            check(diagnostics.Any(message => message.Contains("code_accepted=True")), "diagnostics record verification outcome");
            check(!diagnostics.Any(message => message.Contains("123456") || message.Contains("mock-token")), "diagnostics exclude codes and tokens");
        }
    }
}
