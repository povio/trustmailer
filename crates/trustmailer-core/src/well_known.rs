/// SMTP connection settings for a well-known mail provider, analogous to nodemailer's
/// `well-known/services.json` (`nodemailer.createTransport({ service: "gmail", ... })`).
#[derive(Debug, Clone, Copy)]
pub struct ServiceConfig {
    pub host: &'static str,
    pub port: u16,
    pub secure: bool,
}

/// Looks up a well-known email provider by name, alias, or e-mail domain
/// (case/punctuation-insensitive) — ported from nodemailer's `lib/well-known/services.json`.
/// Accepts a provider name (`"Gmail"`), an alias (`"Google Mail"`), a bare domain
/// (`"gmail.com"`), or a full address (`"user@gmail.com"`, domain part only is used).
pub fn lookup_service(name: &str) -> Option<ServiceConfig> {
    let candidate = name.rsplit('@').next().unwrap_or(name);
    if let Some(cfg) = lookup_service_by_name(candidate) {
        return Some(cfg);
    }
    lookup_service_by_domain(candidate)
}

fn lookup_service_by_name(name: &str) -> Option<ServiceConfig> {
    let key = name.to_ascii_lowercase().replace([' ', '-', '_', '.'], "");
    let cfg = match key.as_str() {
        "1und1" => ServiceConfig {
            host: "smtp.1und1.de",
            port: 465,
            secure: true,
        },
        "126" => ServiceConfig {
            host: "smtp.126.com",
            port: 465,
            secure: true,
        },
        "163" => ServiceConfig {
            host: "smtp.163.com",
            port: 465,
            secure: true,
        },
        "aliyun" => ServiceConfig {
            host: "smtp.aliyun.com",
            port: 465,
            secure: true,
        },
        "aliyunqiye" => ServiceConfig {
            host: "smtp.qiye.aliyun.com",
            port: 465,
            secure: true,
        },
        "aol" => ServiceConfig {
            host: "smtp.aol.com",
            port: 587,
            secure: false,
        },
        "aruba" | "arubapec" => ServiceConfig {
            host: "smtps.aruba.it",
            port: 465,
            secure: true,
        },
        "bluewin" => ServiceConfig {
            host: "smtpauths.bluewin.ch",
            port: 465,
            secure: false,
        },
        "bol" => ServiceConfig {
            host: "smtp.bol.com.br",
            port: 587,
            secure: false,
        },
        "debugmail" => ServiceConfig {
            host: "debugmail.io",
            port: 25,
            secure: false,
        },
        "disroot" => ServiceConfig {
            host: "disroot.org",
            port: 587,
            secure: false,
        },
        "dynect" | "dynectemail" => ServiceConfig {
            host: "smtp.dynect.net",
            port: 25,
            secure: false,
        },
        "elasticemail" => ServiceConfig {
            host: "smtp.elasticemail.com",
            port: 465,
            secure: true,
        },
        "ethereal" | "etherealemail" => ServiceConfig {
            host: "smtp.ethereal.email",
            port: 587,
            secure: false,
        },
        "fastmail" => ServiceConfig {
            host: "smtp.fastmail.com",
            port: 465,
            secure: true,
        },
        "feishu" | "feishumail" => ServiceConfig {
            host: "smtp.feishu.cn",
            port: 465,
            secure: true,
        },
        "fe" | "forwardemail" => ServiceConfig {
            host: "smtp.forwardemail.net",
            port: 465,
            secure: true,
        },
        "gandi" | "gandimail" => ServiceConfig {
            host: "mail.gandi.net",
            port: 587,
            secure: false,
        },
        "gmail" | "googlemail" => ServiceConfig {
            host: "smtp.gmail.com",
            port: 465,
            secure: true,
        },
        "gmailworkspace" | "googleworkspacemail" => ServiceConfig {
            host: "smtp-relay.gmail.com",
            port: 465,
            secure: true,
        },
        "gmx" => ServiceConfig {
            host: "mail.gmx.com",
            port: 587,
            secure: false,
        },
        "godaddy" => ServiceConfig {
            host: "smtpout.secureserver.net",
            port: 25,
            secure: false,
        },
        "godaddyasia" => ServiceConfig {
            host: "smtp.asia.secureserver.net",
            port: 25,
            secure: false,
        },
        "godaddyeurope" => ServiceConfig {
            host: "smtp.europe.secureserver.net",
            port: 25,
            secure: false,
        },
        "hotee" => ServiceConfig {
            host: "mail.hot.ee",
            port: 587,
            secure: false,
        },
        "hotmail" | "hotmailcom" | "outlook" | "outlookcom" => ServiceConfig {
            host: "smtp-mail.outlook.com",
            port: 587,
            secure: false,
        },
        "icloud" | "mac" | "me" => ServiceConfig {
            host: "smtp.mail.me.com",
            port: 587,
            secure: false,
        },
        "infomaniak" => ServiceConfig {
            host: "mail.infomaniak.com",
            port: 587,
            secure: false,
        },
        "kolab" | "kolabnow" => ServiceConfig {
            host: "smtp.kolabnow.com",
            port: 465,
            secure: true,
        },
        "loopia" => ServiceConfig {
            host: "mailcluster.loopia.se",
            port: 465,
            secure: false,
        },
        "loops" => ServiceConfig {
            host: "smtp.loops.so",
            port: 587,
            secure: false,
        },
        "mailee" => ServiceConfig {
            host: "smtp.mail.ee",
            port: 587,
            secure: false,
        },
        "mailru" => ServiceConfig {
            host: "smtp.mail.ru",
            port: 465,
            secure: true,
        },
        "mailcatchapp" => ServiceConfig {
            host: "sandbox-smtp.mailcatch.app",
            port: 2525,
            secure: false,
        },
        "maildev" => ServiceConfig {
            host: "localhost",
            port: 1025,
            secure: false,
        },
        "mailersend" => ServiceConfig {
            host: "smtp.mailersend.net",
            port: 587,
            secure: false,
        },
        "mailgun" => ServiceConfig {
            host: "smtp.mailgun.org",
            port: 465,
            secure: true,
        },
        "mailjet" => ServiceConfig {
            host: "in.mailjet.com",
            port: 587,
            secure: false,
        },
        "mailosaur" => ServiceConfig {
            host: "mailosaur.io",
            port: 25,
            secure: false,
        },
        "mailtrap" => ServiceConfig {
            host: "live.smtp.mailtrap.io",
            port: 587,
            secure: false,
        },
        "mandrill" => ServiceConfig {
            host: "smtp.mandrillapp.com",
            port: 587,
            secure: false,
        },
        "naver" => ServiceConfig {
            host: "smtp.naver.com",
            port: 587,
            secure: false,
        },
        "ohmysmtp" => ServiceConfig {
            host: "smtp.ohmysmtp.com",
            port: 587,
            secure: false,
        },
        "one" => ServiceConfig {
            host: "send.one.com",
            port: 465,
            secure: true,
        },
        "omb" | "openmailbox" | "openmailboxorg" => ServiceConfig {
            host: "smtp.openmailbox.org",
            port: 465,
            secure: true,
        },
        "outlook365" => ServiceConfig {
            host: "smtp.office365.com",
            port: 587,
            secure: false,
        },
        "postmark" | "postmarkapp" => ServiceConfig {
            host: "smtp.postmarkapp.com",
            port: 2525,
            secure: false,
        },
        "proton" | "protonmail" | "protonmailch" | "protonmailcom" | "protonme" => ServiceConfig {
            host: "smtp.protonmail.ch",
            port: 587,
            secure: false,
        },
        "qiyealiyun" => ServiceConfig {
            host: "smtp.mxhichina.com",
            port: 465,
            secure: true,
        },
        "qq" => ServiceConfig {
            host: "smtp.qq.com",
            port: 465,
            secure: true,
        },
        "qqenterprise" | "qqex" => ServiceConfig {
            host: "smtp.exmail.qq.com",
            port: 465,
            secure: true,
        },
        "resend" => ServiceConfig {
            host: "smtp.resend.com",
            port: 465,
            secure: true,
        },
        "runbox" => ServiceConfig {
            host: "smtp.runbox.com",
            port: 465,
            secure: true,
        },
        "sendcloud" => ServiceConfig {
            host: "smtp.sendcloud.net",
            port: 2525,
            secure: false,
        },
        "sendgrid" => ServiceConfig {
            host: "smtp.sendgrid.net",
            port: 587,
            secure: false,
        },
        "brevo" | "sendinblue" => ServiceConfig {
            host: "smtp-relay.brevo.com",
            port: 587,
            secure: false,
        },
        "sendpulse" => ServiceConfig {
            host: "smtp-pulse.com",
            port: 465,
            secure: true,
        },
        "ses" => ServiceConfig {
            host: "email-smtp.us-east-1.amazonaws.com",
            port: 465,
            secure: true,
        },
        "sesapnortheast1" => ServiceConfig {
            host: "email-smtp.ap-northeast-1.amazonaws.com",
            port: 465,
            secure: true,
        },
        "sesapnortheast2" => ServiceConfig {
            host: "email-smtp.ap-northeast-2.amazonaws.com",
            port: 465,
            secure: true,
        },
        "sesapnortheast3" => ServiceConfig {
            host: "email-smtp.ap-northeast-3.amazonaws.com",
            port: 465,
            secure: true,
        },
        "sesapsouth1" => ServiceConfig {
            host: "email-smtp.ap-south-1.amazonaws.com",
            port: 465,
            secure: true,
        },
        "sesapsoutheast1" => ServiceConfig {
            host: "email-smtp.ap-southeast-1.amazonaws.com",
            port: 465,
            secure: true,
        },
        "sesapsoutheast2" => ServiceConfig {
            host: "email-smtp.ap-southeast-2.amazonaws.com",
            port: 465,
            secure: true,
        },
        "sescacentral1" => ServiceConfig {
            host: "email-smtp.ca-central-1.amazonaws.com",
            port: 465,
            secure: true,
        },
        "seseucentral1" => ServiceConfig {
            host: "email-smtp.eu-central-1.amazonaws.com",
            port: 465,
            secure: true,
        },
        "seseunorth1" => ServiceConfig {
            host: "email-smtp.eu-north-1.amazonaws.com",
            port: 465,
            secure: true,
        },
        "seseuwest1" => ServiceConfig {
            host: "email-smtp.eu-west-1.amazonaws.com",
            port: 465,
            secure: true,
        },
        "seseuwest2" => ServiceConfig {
            host: "email-smtp.eu-west-2.amazonaws.com",
            port: 465,
            secure: true,
        },
        "seseuwest3" => ServiceConfig {
            host: "email-smtp.eu-west-3.amazonaws.com",
            port: 465,
            secure: true,
        },
        "sessaeast1" => ServiceConfig {
            host: "email-smtp.sa-east-1.amazonaws.com",
            port: 465,
            secure: true,
        },
        "sesuseast1" => ServiceConfig {
            host: "email-smtp.us-east-1.amazonaws.com",
            port: 465,
            secure: true,
        },
        "sesuseast2" => ServiceConfig {
            host: "email-smtp.us-east-2.amazonaws.com",
            port: 465,
            secure: true,
        },
        "sesusgoveast1" => ServiceConfig {
            host: "email-smtp.us-gov-east-1.amazonaws.com",
            port: 465,
            secure: true,
        },
        "sesusgovwest1" => ServiceConfig {
            host: "email-smtp.us-gov-west-1.amazonaws.com",
            port: 465,
            secure: true,
        },
        "sesuswest1" => ServiceConfig {
            host: "email-smtp.us-west-1.amazonaws.com",
            port: 465,
            secure: true,
        },
        "sesuswest2" => ServiceConfig {
            host: "email-smtp.us-west-2.amazonaws.com",
            port: 465,
            secure: true,
        },
        "seznam" | "seznamemail" => ServiceConfig {
            host: "smtp.seznam.cz",
            port: 465,
            secure: true,
        },
        "smtp2go" => ServiceConfig {
            host: "mail.smtp2go.com",
            port: 2525,
            secure: false,
        },
        "sparkpost" | "sparkpostmail" => ServiceConfig {
            host: "smtp.sparkpostmail.com",
            port: 587,
            secure: false,
        },
        "tipimail" => ServiceConfig {
            host: "smtp.tipimail.com",
            port: 587,
            secure: false,
        },
        "tutanota" => ServiceConfig {
            host: "smtp.tutanota.com",
            port: 465,
            secure: true,
        },
        "yahoo" => ServiceConfig {
            host: "smtp.mail.yahoo.com",
            port: 465,
            secure: true,
        },
        "yandex" => ServiceConfig {
            host: "smtp.yandex.ru",
            port: 465,
            secure: true,
        },
        "zimbra" | "zimbracollaboration" => ServiceConfig {
            host: "smtp.zimbra.com",
            port: 587,
            secure: false,
        },
        "zoho" => ServiceConfig {
            host: "smtp.zoho.com",
            port: 465,
            secure: true,
        },
        _ => return None,
    };
    Some(cfg)
}

fn lookup_service_by_domain(domain: &str) -> Option<ServiceConfig> {
    let key = domain
        .to_ascii_lowercase()
        .replace([' ', '-', '_', '.'], "");
    let cfg = match key.as_str() {
        "aliyuncom" => ServiceConfig {
            host: "smtp.aliyun.com",
            port: 465,
            secure: true,
        },
        "aolcom" => ServiceConfig {
            host: "smtp.aol.com",
            port: 587,
            secure: false,
        },
        "arubait" | "pecarubait" => ServiceConfig {
            host: "smtps.aruba.it",
            port: 465,
            secure: true,
        },
        "bluewinch" => ServiceConfig {
            host: "smtpauths.bluewin.ch",
            port: 465,
            secure: false,
        },
        "bolcombr" => ServiceConfig {
            host: "smtp.bol.com.br",
            port: 587,
            secure: false,
        },
        "disrootorg" => ServiceConfig {
            host: "disroot.org",
            port: 587,
            secure: false,
        },
        "fastmailfm" => ServiceConfig {
            host: "smtp.fastmail.com",
            port: 465,
            secure: true,
        },
        "wwwfeishucn" => ServiceConfig {
            host: "smtp.feishu.cn",
            port: 465,
            secure: true,
        },
        "forwardemailnet" => ServiceConfig {
            host: "smtp.forwardemail.net",
            port: 465,
            secure: true,
        },
        "gmailcom" | "googlemailcom" => ServiceConfig {
            host: "smtp.gmail.com",
            port: 465,
            secure: true,
        },
        "gmxcom" | "gmxde" | "gmxnet" => ServiceConfig {
            host: "mail.gmx.com",
            port: 587,
            secure: false,
        },
        "hotmailcom" | "outlookcom" => ServiceConfig {
            host: "smtp-mail.outlook.com",
            port: 587,
            secure: false,
        },
        "maccom" | "mecom" => ServiceConfig {
            host: "smtp.mail.me.com",
            port: 587,
            secure: false,
        },
        "etikcom" | "ikmailcom" | "ikme" => ServiceConfig {
            host: "mail.infomaniak.com",
            port: 587,
            secure: false,
        },
        "kolabnowcom" => ServiceConfig {
            host: "smtp.kolabnow.com",
            port: 465,
            secure: true,
        },
        "pmme" | "protonmailch" | "protonmailcom" | "protonme" => ServiceConfig {
            host: "smtp.protonmail.ch",
            port: 587,
            secure: false,
        },
        "qqcom" => ServiceConfig {
            host: "smtp.qq.com",
            port: 465,
            secure: true,
        },
        "exmailqqcom" => ServiceConfig {
            host: "smtp.exmail.qq.com",
            port: 465,
            secure: true,
        },
        "runboxcom" => ServiceConfig {
            host: "smtp.runbox.com",
            port: 465,
            secure: true,
        },
        "emailcz" | "postcz" | "seznamcz" | "spoluzacicz" => ServiceConfig {
            host: "smtp.seznam.cz",
            port: 465,
            secure: true,
        },
        "sparkpostcom" => ServiceConfig {
            host: "smtp.sparkpostmail.com",
            port: 587,
            secure: false,
        },
        "tutacom" | "tutaio" | "tutanotacom" | "tutanotade" => ServiceConfig {
            host: "smtp.tutanota.com",
            port: 465,
            secure: true,
        },
        "yahoocom" => ServiceConfig {
            host: "smtp.mail.yahoo.com",
            port: 465,
            secure: true,
        },
        "yandexru" => ServiceConfig {
            host: "smtp.yandex.ru",
            port: 465,
            secure: true,
        },
        _ => return None,
    };
    Some(cfg)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_by_key() {
        let cfg = lookup_service("Gmail").unwrap();
        assert_eq!(cfg.host, "smtp.gmail.com");
        assert_eq!(cfg.port, 465);
        assert!(cfg.secure);
    }

    #[test]
    fn finds_by_alias() {
        let cfg = lookup_service("Google Mail").unwrap();
        assert_eq!(cfg.host, "smtp.gmail.com");
    }

    #[test]
    fn finds_by_domain() {
        let cfg = lookup_service("GoogleMail.com").unwrap();
        assert_eq!(cfg.host, "smtp.gmail.com");
    }

    #[test]
    fn finds_by_bare_domain() {
        let cfg = lookup_service("gmail.com").unwrap();
        assert_eq!(cfg.host, "smtp.gmail.com");
    }

    #[test]
    fn finds_by_full_email_address() {
        let cfg = lookup_service("user@gmail.com").unwrap();
        assert_eq!(cfg.host, "smtp.gmail.com");
    }

    #[test]
    fn is_case_and_punctuation_insensitive() {
        assert!(lookup_service("GMAIL").is_some());
        assert!(lookup_service("google-mail").is_some());
    }

    #[test]
    fn finds_no_match() {
        assert!(lookup_service("zzzzzz").is_none());
    }

    #[test]
    fn covers_common_providers() {
        for name in [
            "gmail",
            "outlook",
            "yahoo",
            "icloud",
            "zoho",
            "sendgrid",
            "mailgun",
            "ses",
            "fastmail",
            "protonmail",
        ] {
            assert!(
                lookup_service(name).is_some(),
                "missing well-known service: {name}"
            );
        }
    }
}
