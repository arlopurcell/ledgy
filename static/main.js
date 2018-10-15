const API_BASE_URL = "/api/";
var currentAccount = "spend";

function selectAccount(account) {
    currentAccount = account;
    reloadTransactions();
    showTransactionFormPage(1);
}

function showTransactionFormPage(page) {
    $(".transactionFormPage").hide();
    $("#transactionForm" + page).show();
    if (page == 1) {
        $("#amountInput").focus();
    } else if (page == 2) {
        $("#descriptionInput").focus();
    }
}

function sendTransaction() {
    var type = $("input[name='transType']:checked").val();
    var amount = $("#amountInput").val();
    var description = $("#descriptionInput").val();

    // disable buttons
    $(".transactionButton").prop("disabled", true);
    
    var auth = window.localStorage.getItem("authorization");
    if (!auth) {
        $("#credsModal").modal();
        return
    }
    
    // ajax do transaction
    fetch(API_BASE_URL + currentAccount + "/" + type, {
        method: 'POST',
        headers: {
            'Content-Type': 'application/json',
            "Authorization": auth,
        },
        body: JSON.stringify({amount: Math.trunc(amount * 100), description: description}),
    }).then(function(response) {
        if (response.ok) {
            $(`#${type}s-tab`).tab("show");
            reloadTransactions();
            $("#amountInput").val("");
            $("#descriptionInput").val("");
            showTransactionFormPage(1);
            $(".transactionButton").prop("disabled", false);
        } else {
            showTransactionError();
        }
    }).catch(function(error) {
        showTransactionError();
    });
}

function showTransactionError() {
    $("#alert-container").empty();
    $("#alert-container").html(`
        <div class="alert alert-danger alert-dismissible fade show" role="alert">
            Failed to submit transaction. Please try again.
            <button type="button" class="close" data-dismiss="alert" aria-label="Close">
                <span aria-hidden="true">&times;</span>
            </button>
        </div>
    `);
    $(".transactionButton").prop("disabled", false);
}

function reloadTransactions() {
    $(".loader").show();

    var auth = window.localStorage.getItem("authorization");
    if (!auth) {
        $("#credsModal").modal();
        return
    }

    // ajax call
    fetch(API_BASE_URL + currentAccount, {
        method: 'GET',
        headers: {
            'Content-Type': 'application/json',
            "Authorization": auth,
        },
    }).then(function(response) {
        response.json().then(function(data) {
            // update balance
            $("#balanceSpan").html("$" + (data.balance / 100).toFixed(2));
            //$("#balanceSpan").html("$" + data.balance.slice(0, -2) + "." + data.balance.slice(-2));
            // replace contents of $("#transactionContainer)
            var container = $("#depositsTableBody");
            container.empty();
            data.credits.forEach(function(transaction) {
                var row = jQuery('<tr/>');
                row.appendTo(container);
                var amount_part = jQuery('<td/>', {class: "transaction-amount"}); 
                amount_part.appendTo(row);
                amount_part.text("$" + (transaction.amount / 100).toFixed(2));
                var description_part = jQuery('<td/>', {class: "transaction-description"}); 
                description_part.appendTo(row);
                description_part.text(transaction.description);
            });

            container = $("#withdrawalsTableBody");
            container.empty();
            data.debits.forEach(function(transaction) {
                var row = jQuery('<tr/>');
                row.appendTo(container);
                var amount_part = jQuery('<td/>', {class: "transaction-amount"}); 
                amount_part.appendTo(row);
                var amount = transaction.amount * -1;
                amount_part.text("$" + (amount / 100).toFixed(2));
                var description_part = jQuery('<td/>', {class: "transaction-description"}); 
                description_part.appendTo(row);
                description_part.text(transaction.description);
            });
            $(".loader").hide();
        });
    });
}

function reloadAccounts() {
    var auth = window.localStorage.getItem("authorization");
    if (!auth) {
        $("#credsModal").modal();
        return
    }

    fetch(API_BASE_URL + "list", {
        method: 'GET',
        headers: {
            'Content-Type': 'application/json',
            "Authorization": auth,
        },
    }).then(function(response) {
        response.json().then(function(data) {
            var accounts = data.ledgers.reverse();
            window.localStorage.setItem("accounts", accounts);
            updateAccountsDisplay();
        });
    });
}

function updateAccountsDisplay() {
    var accountsString = window.localStorage.getItem("accounts");
    var accounts = accountsString.split(",");
    currentAccount = accounts[0];
    var container = $("#accountButtonContainer");
    container.empty();
    accounts.forEach(function(account) {
        var label = jQuery("<label/>", {
            class: "btn btn-secondary",
            onclick: `selectAccount('${account}');`,
        });
        label.appendTo(container);
        var input = jQuery("<input>", {
            type: "radio",
            name: "account",
            autocomplete: "off",
        });
        input.appendTo(label);
        label.append(account);
        if (account == currentAccount) {
            label.addClass("active");
            input.prop("checked", true);
        }
    });
    // TODO add "+" button
}

function saveCredentials() {
    var username = $("#usernameInput").val();
    var password = $("#passwordInput").val();
    $("#usernameInput").val("");
    $("#passwordInput").val("");
    window.localStorage.setItem("authorization", "Basic " + window.btoa(`${username}:${password}`));
    // TODO close modal if necessary
    reloadAccounts();
    reloadTransactions();
}

function keypadType(input) {
    $("#amountInput").val($("#amountInput").val() + input);
}

function keypadDelete() {
    var amountInput = $("#amountInput");
    var current = amountInput.val();
    if (current.length > 0) {
        amountInput.val(current.substring(0, current.length - 1));
    }
}

function keypadEnter(transType) {
    $(`input[name='transType'][value='${transType}']`).prop("checked", true);
    showTransactionFormPage(2);
}

window.onload = function() {
    var accounts = window.localStorage.getItem("accounts");
    if (accounts == null) {
        accounts = ["spend,saved"];
        window.localStorage.setItem("accounts", accounts);
    }
    updateAccountsDisplay();

    reloadAccounts();
    reloadTransactions();
    $("#amountInput").focus();

    if ('serviceWorker' in navigator) {
        navigator.serviceWorker
                 .register('./service-worker.js')
                 .then(function() { console.log('Service Worker Registered'); });
    }
};
